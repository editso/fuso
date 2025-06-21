#ifdef _WIN32
#include <windows.h>
#include <consoleapi.h>
#include <libloaderapi.h>

#include <stdio.h>

#else
#include <stdlib.h>
#include <errno.h>
#include <fcntl.h>
#include <pty.h>
#include <signal.h>
#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wait.h>
#endif

#include "pty_impl.h"

#ifdef _WIN32

typedef HRESULT (*WINAPI CreatePseudoConsoleFn)(COORD size, HANDLE hInput, HANDLE hOutput,
                                                DWORD dwFlags, HPCON *phPC);

typedef HRESULT (*WINAPI ResizePseudoConsoleFn)(HPCON hPC, COORD size);

typedef void (*WINAPI ClosePseudoConsoleFn)(HPCON hPC);

static struct {
  HMODULE               module;
  ClosePseudoConsoleFn  close_pseudo_console;
  CreatePseudoConsoleFn create_pseudo_console;
  ResizePseudoConsoleFn resize_pseudo_console;
} conpty_api = {.module                = NULL,
                .resize_pseudo_console = NULL,
                .create_pseudo_console = NULL,
                .resize_pseudo_console = NULL};

int conpty_init();

void clean_process_startup_info(STARTUPINFOEX *si);

int setup_pseudo_console(pty_process *process, STARTUPINFOEX *psi, COORD size);

int pty_spawn(pty_process *process) {
  STARTUPINFOEX      *psi;
  PROCESS_INFORMATION pi;
  COORD               size;
  HANDLE              hJob;
  HANDLE              hWait;
  HANDLE              inputReadSide;
  HANDLE              outputWriteSide;

  if (conpty_init() != 0)
    return -1;

  size.X = 100;
  size.Y = 100;

  hWait = NULL;
  psi   = (STARTUPINFOEX *)VirtualAlloc(NULL, sizeof(STARTUPINFOEX), MEM_COMMIT | MEM_RESERVE,
                                        PAGE_READWRITE);

  if (!psi) {
    return -1;
  }

  if (setup_pseudo_console(process, psi, size) != 0) {
    goto error;
  }

  ZeroMemory(&pi, sizeof(pi));

  hJob                                      = CreateJobObject(NULL, NULL);
  JOBOBJECT_EXTENDED_LIMIT_INFORMATION jeli = {0};
  jeli.BasicLimitInformation.LimitFlags     = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

  SetInformationJobObject(hJob, JobObjectExtendedLimitInformation, &jeli, sizeof(jeli));
  AssignProcessToJobObject(hJob, GetCurrentProcess());

  SetConsoleCtrlHandler(NULL, FALSE);

  if (!CreateProcessA(NULL, (LPSTR)process->cmdline, NULL, NULL, FALSE,
                      EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
                      (LPVOID)process->environs, (LPCSTR)process->work, &psi->StartupInfo, &pi)) {
    goto error;
  }

  if (process->exit_cb) {
    if (!RegisterWaitForSingleObject(&hWait, pi.hProcess, process->exit_cb, process->exit_cb_data,
                                     INFINITE, WT_EXECUTEONLYONCE)) {
      goto error;
    }
  }


  process->wait         = hWait;
  process->pid          = pi.dwProcessId;
  process->handle       = pi.hProcess;
  process->startup_info = psi;

  ResumeThread(pi.hThread);
  CloseHandle(pi.hThread);

  AssignProcessToJobObject(hJob, pi.hProcess);

  return 0;

error:

  clean_process_startup_info(psi);

  return HRESULT_FROM_WIN32(GetLastError());
}

void pty_exit(pty_process *process) {
  if (!process || conpty_init() != 0)
    return;

  if (process->handle) {

    if (process->wait) {
      UnregisterWait(process->wait);
      process->wait = NULL;
    }

    clean_process_startup_info(process->startup_info);
    TerminateProcess((HANDLE)process->handle, 0);
    CloseHandle(process->handle);
  }

  if (process->pty) {
    conpty_api.close_pseudo_console(process->pty);
  }

  process->pty          = NULL;
  process->handle       = NULL;
  process->startup_info = NULL;
}

void clean_process_startup_info(STARTUPINFOEX *si) {
  if (!si)
    return;

  if (si->lpAttributeList != NULL) {
    DeleteProcThreadAttributeList(si->lpAttributeList);
  }

  VirtualFree(si, 0, MEM_RELEASE);
}

int setup_pseudo_console(pty_process *process, STARTUPINFOEX *si, COORD size) {
  HPCON   hPC = INVALID_HANDLE_VALUE;
  HRESULT hr  = S_OK;

  CHAR inputPipeName[255];
  CHAR outputPipeName[255];

  HANDLE pInputReadSide   = INVALID_HANDLE_VALUE;
  HANDLE pOutputWriteSide = INVALID_HANDLE_VALUE;

  SECURITY_ATTRIBUTES sa = {0};

  size_t bytesRequired;

  DWORD pipeMode     = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT;
  DWORD pipeOpenMode = PIPE_ACCESS_INBOUND | PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE;

  ZeroMemory(inputPipeName, sizeof(inputPipeName));
  ZeroMemory(outputPipeName, sizeof(outputPipeName));

  if (!process->pipe_output_name || !process->pipe_input_name) {
    return -1;
  }

  sprintf(inputPipeName, "\\\\.\\pipe\\%s", process->pipe_input_name);
  sprintf(outputPipeName, "\\\\.\\pipe\\%s", process->pipe_output_name);

  pInputReadSide   = CreateNamedPipeA(inputPipeName, pipeOpenMode, pipeMode, 1, 0, 0, 30000, &sa);
  pOutputWriteSide = CreateNamedPipeA(outputPipeName, pipeOpenMode, pipeMode, 1, 0, 0, 30000, &sa);

  if (pInputReadSide == INVALID_HANDLE_VALUE || pOutputWriteSide == INVALID_HANDLE_VALUE) {
    goto cleanup;
  }

  hr = conpty_api.create_pseudo_console(size, pInputReadSide, pOutputWriteSide, 0, &hPC);

  if (FAILED(hr)) {
    goto cleanup;
  }

  ZeroMemory(si, sizeof(STARTUPINFOEX));

  si->StartupInfo.cb = sizeof(STARTUPINFOEX);
  si->StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
  si->StartupInfo.hStdError  = NULL;
  si->StartupInfo.hStdInput  = NULL;
  si->StartupInfo.hStdOutput = NULL;

  InitializeProcThreadAttributeList(NULL, 1, 0, &bytesRequired);

  si->lpAttributeList = (PPROC_THREAD_ATTRIBUTE_LIST)HeapAlloc(GetProcessHeap(), 0, bytesRequired);

  if (!si->lpAttributeList) {
    goto cleanup;
  }

  if (!InitializeProcThreadAttributeList(si->lpAttributeList, 1, 0, &bytesRequired)) {
    HeapFree(GetProcessHeap(), 0, si->lpAttributeList);
    goto cleanup;
  }

  if (!UpdateProcThreadAttribute(si->lpAttributeList, 0, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, hPC,
                                 sizeof(hPC), NULL, NULL)) {
    HeapFree(GetProcessHeap(), 0, si->lpAttributeList);
    goto cleanup;
  }

  process->pty = hPC;

cleanup:

  if (pInputReadSide != INVALID_HANDLE_VALUE)
    CloseHandle(pInputReadSide);
  if (pOutputWriteSide != INVALID_HANDLE_VALUE)
    CloseHandle(pOutputWriteSide);

  return S_OK;
}


int conpty_init() {

  if (!conpty_api.module) {
    conpty_api.module = LoadLibraryA("kernel32.dll");
    if (!conpty_api.module)
      return -GetLastError();
  }

  if (!conpty_api.close_pseudo_console) {
    conpty_api.close_pseudo_console =
        (ClosePseudoConsoleFn)GetProcAddress(conpty_api.module, "ClosePseudoConsole");
  }

  if (!conpty_api.create_pseudo_console) {
    conpty_api.create_pseudo_console =
        (CreatePseudoConsoleFn)GetProcAddress(conpty_api.module, "CreatePseudoConsole");
  }

  if (!conpty_api.resize_pseudo_console) {
    conpty_api.resize_pseudo_console =
        (ResizePseudoConsoleFn)GetProcAddress(conpty_api.module, "ResizePseudoConsole");
  }

  return 0;
}


#else

void enter_pty_main(const char *cwd, char *const *argv, const char **envp);

int pty_spawn(pty_process *process) {
  int pid;
  int master;
  int flags;
  int status;

  struct winsize size = {.ws_row = 100, .ws_col = 100, 0, 0};

  pid = forkpty(&master, NULL, NULL, &size);

  if (pid < 0) {
    return -errno;
  } else if (pid == 0) {
    enter_pty_main(process->work, process->argv, process->environs);
  }

  if ((flags = fcntl(master, F_GETFL)) == -1) {
    status = -errno;
    goto exit_pty;
  }

  if (fcntl(master, F_SETFL, flags | O_NONBLOCK) == -1) {
    status = -errno;
    goto exit_pty;
  }

  process->pid = pid;
  process->pty = master;

  return 0;

exit_pty:

  close(master);

  kill(pid, SIGKILL);

  waitpid(pid, NULL, 0);

  return status;
}

void pty_exit(pty_process *process) {
  if (!process)
    return;

  if (process->pty) {
    close(process->pty);
  }

  kill(process->pid, SIGKILL);
  waitpid(process->pid, NULL, 0);

  process->pty = -1;
  process->pid = -1;
}

void enter_pty_main(const char *cwd, char *const *argv, const char **envp) {

  if (cwd != NULL)
    chdir(cwd);

  if (envp != NULL) {
    clearenv();
    const char **p = envp;
    for (; *p; p++) putenv((char *)*p);
  }

  int ret = execvp(argv[0], argv);
  if (ret < 0) {
    _exit(-errno);
  }
}

#endif