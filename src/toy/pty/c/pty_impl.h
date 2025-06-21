#ifndef __TTY_H__
#define __TTY_H__

typedef struct _pty_process {
  int pid;

#ifndef _WIN32
  int          pty;
  char *const *argv;
  const char **environs;
#else
  void                 *pty;
  const char           *cmdline;
  const unsigned short *environs;
#endif

  const char *work;

#ifdef _WIN32
  void       *wait;
  void       *handle;
  void       *startup_info;
  const char *pipe_input_name;
  const char *pipe_output_name;
  void        (*exit_cb)(void *context, unsigned char);
  void       *exit_cb_data;

#endif

} pty_process;

int pty_spawn(pty_process *process);

void pty_exit(pty_process *process);

#endif