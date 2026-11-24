//! A client end of an in-memory connection to a server, for wire tests.

use super::serve::{run, runtime, serve};
use crate::error::Result;
use crate::server::Server;
use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tokio::io::{
    AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, DuplexStream, ReadHalf, WriteHalf,
};

/// How many bytes the in-memory stream buffers in each direction.
const BUFFER: usize = 1 << 20;

/// The client end: raw lines in, raw lines out, each wait bounded.
pub(crate) struct DuplexClient {
    runtime: tokio::runtime::Runtime,
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
    served: mpsc::Receiver<Result<()>>,
    deadline: Duration,
}

impl DuplexClient {
    /// Serve `server` on its own thread and runtime, over an in-memory
    /// stream whose other end this client holds.
    pub(crate) fn start<S: Send + Sync + 'static>(
        server: Server<S>,
        deadline: Duration,
    ) -> Result<Self> {
        let (client, server_end) = tokio::io::duplex(BUFFER);
        let (done, served) = mpsc::channel();
        thread::spawn(move || {
            let (input, output) = tokio::io::split(server_end);
            let result = run(|| serve(server, input, output));
            // The client may be gone already; then nobody is waiting.
            let _ = done.send(result);
        });
        let (reader, writer) = tokio::io::split(client);
        Ok(Self {
            runtime: runtime()?,
            reader: BufReader::new(reader),
            writer,
            served,
            deadline,
        })
    }

    /// Write `line` and a newline.
    pub(crate) fn send_line(&mut self, line: &str) -> io::Result<()> {
        let deadline = self.deadline;
        let writer = &mut self.writer;
        self.runtime.block_on(async {
            let write = async {
                writer.write_all(line.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await
            };
            tokio::time::timeout(deadline, write)
                .await
                .map_err(|_| timed_out("writing a line"))?
        })
    }

    /// The next line the server wrote, without its newline.
    pub(crate) fn read_line(&mut self) -> io::Result<String> {
        let deadline = self.deadline;
        let reader = &mut self.reader;
        self.runtime.block_on(async {
            let mut line = String::new();
            let read = tokio::time::timeout(deadline, reader.read_line(&mut line))
                .await
                .map_err(|_| timed_out("waiting for a line"))??;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the server closed its output",
                ));
            }
            Ok(line.trim_end_matches(['\r', '\n']).to_owned())
        })
    }

    /// Close the client's output, which is the server's input, and wait for
    /// the server to return.
    pub(crate) fn close(mut self) -> io::Result<Result<()>> {
        let deadline = self.deadline;
        let writer = &mut self.writer;
        self.runtime.block_on(async {
            tokio::time::timeout(deadline, writer.shutdown())
                .await
                .map_err(|_| timed_out("closing the input"))?
        })?;
        self.served
            .recv_timeout(deadline)
            .map_err(|_| timed_out("waiting for the server to return"))
    }
}

fn timed_out(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, format!("timed out {what}"))
}
