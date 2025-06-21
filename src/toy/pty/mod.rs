mod c;

use std::{collections::HashMap, ffi::CString, pin::Pin, sync::Arc};

use c::bindings;

#[cfg(windows)]
pub use c::windows_ext::*;
#[cfg(windows)]
use std::task::Poll;

#[cfg(windows)]
use crate::core::task::{setter, Getter, Setter};

use parking_lot::Mutex;

use crate::{
    core::io::{AsyncRead, AsyncWrite},
    error,
};

extern "C" {
    pub fn read(fd: i32, buf: *mut u8, nbyte: usize) -> isize;
}

#[derive(Debug)]
#[allow(unused)]
struct Process {
    #[cfg(not(windows))]
    args: Vec<*const u8>,

    #[cfg(windows)]
    cmdline: *const u8,

    #[cfg(not(windows))]
    environs: Vec<*const u8>,

    #[cfg(windows)]
    environs: Vec<u16>,

    work_dir: *const u8,

    #[cfg(windows)]
    pipe_input_name: *const u8,
    #[cfg(windows)]
    pipe_output_name: *const u8,
}

#[allow(unused)]
pub struct Pty {
    pty: bindings::PtyProcess,
    proc: Process,
    #[cfg(windows)]
    pipe_input_name: CString,
    #[cfg(windows)]
    pipe_output_name: CString,
    work: Option<CString>,
    #[cfg(not(windows))]
    argv: Vec<CString>,
    #[cfg(not(windows))]
    environs: Vec<CString>,
    #[cfg(windows)]
    environs: Vec<String>,
    #[cfg(windows)]
    cmdline: CString,
    #[cfg(windows)]
    input_signal: Getter<()>,
    #[cfg(windows)]
    output_signal: Getter<()>,
}

pub struct OpenedPty {
    pty: Arc<Mutex<Pty>>,
    input: Arc<Mutex<AbstractPtyInput>>,
    output: Arc<Mutex<AbstractPtyOutput>>,
}

#[derive(Debug)]
pub struct PtyBuilder {
    program: String,
    arguments: Vec<String>,
    work_dir: Option<String>,
    environs: HashMap<String, String>,
    inherit_parent_env: bool,
}

pub trait PtyInput: AsyncWrite {}

pub trait PtyOutput: AsyncRead {}

pub struct AbstractPtyInput(Box<dyn PtyInput + Send + Sync + Unpin + 'static>);

pub struct AbstractPtyOutput(Box<dyn PtyOutput + Send + Sync + Unpin + 'static>);

#[cfg(windows)]
pub trait OpenPty {
    fn connect_input(input: String) -> error::Result<AbstractPtyInput>;
    fn connect_output(output: String) -> error::Result<AbstractPtyOutput>;
}

#[cfg(not(windows))]
pub trait OpenPty {
    fn pipe(input: i32) -> error::Result<(AbstractPtyInput, AbstractPtyOutput)>;
}

impl Pty {
    fn open(
        prog: String,
        work: Option<String>,
        args: Option<Vec<String>>,
        envp: Option<HashMap<String, String>>,
    ) -> error::Result<Self> {
        unsafe {
            let mut pty = std::mem::zeroed::<bindings::PtyProcess>();

            let work = match work {
                Some(work) => Some(CString::new(work)?),
                None => None,
            };

            #[cfg(not(windows))]
            let argv = {
                let mut argv = Vec::<CString>::new();

                argv.push(CString::new(prog)?);

                if let Some(args) = args {
                    for arg in args {
                        argv.push(CString::new(arg)?);
                    }
                }

                argv
            };

            #[cfg(not(windows))]
            let environs = match envp {
                None => Vec::new(),
                Some(envp) => envp.into_iter().fold(Vec::new(), |mut envp, (k, v)| {
                    envp.push(CString::new(format!("{k}={v}")).unwrap());
                    envp
                }),
            };

            #[cfg(windows)]
            let environs = {
                match envp {
                    None => vec![],
                    Some(envp) => envp.into_iter().fold(Vec::new(), |mut envp, (k, v)| {
                        envp.push(format!("{k}={v}"));
                        envp
                    }),
                }
            };

            #[cfg(windows)]
            let cmdline = {
                let mut argv = vec![prog];
                CString::new(
                    match args {
                        None => argv,
                        Some(args) => {
                            argv.extend(args);
                            argv
                        }
                    }
                    .join(" "),
                )?
            };

            #[cfg(windows)]
            let pipe_input_name = CString::new(format!("pty-fuso-input-{}", std::process::id()))?;

            #[cfg(windows)]
            let pipe_output_name = CString::new(format!("pty-fuso-output-{}", std::process::id()))?;

            let proc = Process {
                #[cfg(not(windows))]
                args: {
                    let mut args = vec![];
                    args.extend(argv.iter().map(|arg| arg.as_ptr() as *const u8));
                    args.push(std::ptr::null());
                    args
                },
                #[cfg(not(windows))]
                environs: {
                    let mut envs = vec![];
                    envs.extend(environs.iter().map(|env| env.as_ptr() as *const u8));
                    envs.push(std::ptr::null());
                    envs
                },
                #[cfg(windows)]
                environs: {
                    if environs.is_empty() {
                        vec![]
                    } else {
                        let mut envp = environs.iter().fold(Vec::new(), |mut envs, env| {
                            envs.extend(env.encode_utf16().collect::<Vec<u16>>());
                            envs.push(0);
                            envs
                        });
                        envp.push(0);
                        envp
                    }
                },
                work_dir: {
                    work.as_ref()
                        .map(|work| work.as_ptr() as _)
                        .unwrap_or(std::ptr::null())
                },
                #[cfg(windows)]
                cmdline: cmdline.as_ptr() as _,
                #[cfg(windows)]
                pipe_input_name: pipe_input_name.as_ptr() as _,
                #[cfg(windows)]
                pipe_output_name: pipe_output_name.as_ptr() as _,
            };

            #[cfg(not(windows))]
            {
                pty.argv = proc.args.as_ptr() as _;
                pty.environs = proc.environs.as_ptr() as _;
            }

            #[cfg(not(windows))]
            {
                pty.work = proc.work_dir as _;
            }

            #[cfg(windows)]
            {
                pty.work_dir = proc.work_dir as _;
                pty.cmdline = proc.cmdline as _;
                if proc.environs.is_empty() {
                    pty.environs = std::ptr::null();
                } else {
                    pty.environs = proc.environs.as_ptr() as _;
                }

                pty.pipe_input_name = proc.pipe_input_name;
                pty.pipe_output_name = proc.pipe_output_name;
                fn pty_process_exit_cb(data: *mut std::os::raw::c_void) {
                    unsafe {
                        drop(Box::<(Setter<()>, Setter<()>)>::from_raw(
                            data as *mut (Setter<()>, Setter<()>),
                        ));
                    }
                }

                pty.exit_cb = pty_process_exit_cb as _;
            }

            #[cfg(windows)]
            let (set_input_signal, input_signal) = setter::<()>();

            #[cfg(windows)]
            let (set_output_signal, output_signal) = setter::<()>();

            #[cfg(windows)]
            {
                pty.exit_cb_data =
                    Box::into_raw(Box::new((set_input_signal, set_output_signal))) as _;
            }

            let error = bindings::pty_spawn(&mut pty as *mut bindings::PtyProcess);

            if error < 0 {
                Err(std::io::Error::from_raw_os_error(error).into())
            } else {
                Ok(Self {
                    pty,
                    proc,
                    work,
                    #[cfg(not(windows))]
                    argv,
                    #[cfg(windows)]
                    cmdline,
                    environs,
                    #[cfg(windows)]
                    pipe_input_name,
                    #[cfg(windows)]
                    pipe_output_name,
                    #[cfg(windows)]
                    input_signal,
                    #[cfg(windows)]
                    output_signal,
                })
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        unsafe { bindings::pty_exit(&mut self.pty as *mut bindings::PtyProcess) }
    }
}

pub fn builder<P: ToString>(program: P) -> PtyBuilder {
    PtyBuilder {
        program: program.to_string(),
        arguments: vec![],
        work_dir: None,
        environs: Default::default(),
        inherit_parent_env: true,
    }
}

impl AbstractPtyInput {
    pub fn new<I>(input: I) -> Self
    where
        I: PtyInput + Send + Sync + Unpin + 'static,
    {
        Self(Box::new(input))
    }
}

impl AbstractPtyOutput {
    pub fn new<I>(output: I) -> Self
    where
        I: PtyOutput + Send + Sync + Unpin + 'static,
    {
        Self(Box::new(output))
    }
}

impl PtyBuilder {
    pub fn arg<A: ToString>(&mut self, arg: A) -> &mut Self {
        self.arguments.push(arg.to_string().trim().to_owned());
        self
    }

    pub fn args<A: ToString>(&mut self, args: &[A]) -> &mut Self {
        for arg in args {
            self.arguments.push(arg.to_string().trim().to_owned());
        }
        self
    }

    pub fn work_dir<A: ToString>(&mut self, work_dir: A) -> &mut Self {
        self.work_dir.replace(work_dir.to_string());
        self
    }

    pub fn env<K: ToString, V: ToString>(&mut self, key: K, value: V) -> &mut Self {
        self.environs.insert(key.to_string(), value.to_string());
        self
    }

    pub fn envs<K: ToString, V: ToString>(&mut self, envs: HashMap<K, V>) -> &mut Self {
        for (k, v) in envs {
            self.environs.insert(k.to_string(), v.to_string());
        }
        self
    }

    pub fn inherit_parent_env(&mut self, inherit: bool) -> &mut Self {
        self.inherit_parent_env = inherit;
        self
    }

    pub fn build_with<O>(mut self) -> error::Result<OpenedPty>
    where
        O: OpenPty,
    {
        if !self.environs.is_empty() && self.inherit_parent_env {
            for (k, v) in std::env::vars() {
                if !self.environs.contains_key(&k) {
                    self.env(k, v);
                }
            }
        }

        let pty = Pty::open(
            self.program,
            self.work_dir,
            Some(self.arguments),
            Some(self.environs),
        )?;

        #[cfg(windows)]
        {
            let input = format!(r"\\.\\pipe\\{}", pty.pipe_input_name.to_str()?);
            let output = format!(r"\\.\\pipe\\{}", pty.pipe_output_name.to_str()?);

            let input = O::connect_input(input)?;
            let output = O::connect_output(output)?;

            Ok(OpenedPty {
                pty: Arc::new(Mutex::new(pty)),
                input: Arc::new(Mutex::new(input)),
                output: Arc::new(Mutex::new(output)),
            })
        }

        #[cfg(not(windows))]
        {
            let (input, output) = O::pipe(pty.pty.pty)?;

            Ok(OpenedPty {
                pty: Arc::new(Mutex::new(pty)),
                input: Arc::new(Mutex::new(input)),
                output: Arc::new(Mutex::new(output)),
            })
        }
    }
}

impl Clone for OpenedPty {
    fn clone(&self) -> Self {
        Self {
            pty: self.pty.clone(),
            input: self.input.clone(),
            output: self.output.clone(),
        }
    }
}

unsafe impl Sync for OpenedPty {}
unsafe impl Send for OpenedPty {}

impl<T> PtyInput for T where T: AsyncWrite + Unpin {}
impl<T> PtyOutput for T where T: AsyncRead + Unpin {}

#[cfg(windows)]
impl AsyncRead for OpenedPty {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut pty = self.pty.lock();
        match std::future::Future::poll(Pin::new(&mut pty.output_signal), cx)? {
            Poll::Ready(()) => Poll::Ready(Ok(0)),
            Poll::Pending => {
                let mut output = self.output.lock();
                Pin::new(&mut *output.0).poll_read(cx, buf)
            }
        }
    }
}

#[cfg(windows)]
impl AsyncWrite for OpenedPty {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut pty = self.pty.lock();
        match std::future::Future::poll(Pin::new(&mut pty.input_signal), cx)? {
            Poll::Ready(()) => Poll::Ready(Ok(0)),
            Poll::Pending => {
                let mut input = self.input.lock();
                Pin::new(&mut *input.0).poll_write(cx, buf)
            }
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<()>> {
        let mut pty = self.pty.lock();
        match std::future::Future::poll(Pin::new(&mut pty.input_signal), cx)? {
            Poll::Ready(()) => Poll::Ready(Ok(())),
            Poll::Pending => {
                let mut input = self.input.lock();
                Pin::new(&mut *input.0).poll_flush(cx)
            }
        }
    }
}

#[cfg(not(windows))]
impl AsyncRead for OpenedPty {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut output = self.output.lock();
        Pin::new(&mut *output.0).poll_read(cx, buf)
    }
}

#[cfg(not(windows))]
impl AsyncWrite for OpenedPty {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut input = self.input.lock();
        Pin::new(&mut *input.0).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<()>> {
        let mut input = self.input.lock();
        Pin::new(&mut *input.0).poll_flush(cx)
    }
}

#[cfg(test)]
#[cfg(windows)]
#[cfg(feature = "fuso-rt-tokio")]
mod tests {

    use crate::{
        core::{io, split::SplitStream},
        pty::c::windows_ext::ConsoleExt,
    };

    #[tokio::test]
    async fn test_pty() {
        let (output, input) = super::builder("C:\\windows\\system32\\cmd.exe")
            .build()
            .expect("Failed to open pty")
            .split();

        tokio::io::stdin().enable_input_mode().unwrap();

        tokio::select! {
            _ = io::copy(input, tokio::io::stdin()) => {}
            _ = io::copy(tokio::io::stdout(), output) => {}
        };
    }
}

#[cfg(test)]
#[cfg(not(windows))]
#[cfg(feature = "fuso-rt-tokio")]
mod tests {

    use crate::core::{io, split::SplitStream};

    #[tokio::test]
    async fn test_pty() {
        let mut builder = super::builder("/bin/bash");

        builder
            .work_dir("/")
            .inherit_parent_env(true)
            .env("aa", "1")
            .env("C", "C");

        let (output, input) = builder.build().expect("Failed to open pty").split();

        tokio::select! {
            e = io::copy(input, tokio::io::stdin()) => {
                println!("{e:?}")
            }
            e = io::copy(tokio::io::stdout(), output) => {
                println!("{e:?}")
            }
        };
    }
}
