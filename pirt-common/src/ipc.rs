use std::{
    collections::HashMap,
    io::{self, BufReader, BufWriter, ErrorKind, Read, Write},
    num::NonZero,
    path::Path,
};

use bincode::{Decode, Encode, config::Config, error::DecodeError};
use interprocess::os::windows::{
    ToWtf16,
    named_pipe::{
        DuplexPipeStream, PipeListener, PipeListenerOptions, PipeMode, PipeStream, RecvPipeStream,
        SendPipeStream,
        pipe_mode::{self, Bytes, Messages},
    },
    security_descriptor::SecurityDescriptor,
};

const ALLOW_ALL: &'static str = "D:(A;;GA;;;WD)(A;;GA;;;AN)(A;;GA;;;AU)(A;;GA;;;BA)(A;;GA;;;SY)(A;;GA;;;CO)(A;;GA;;;PS)(A;;GA;;;IU)(A;;GA;;;SU)(A;;GA;;;RC)(A;;GA;;;WR)";

fn create_listener(path: &Path) -> PipeListener<Bytes, Bytes> {
    PipeListenerOptions::new()
        .path(path)
        .accept_remote(false)
        .write_through(false)
        .inheritable(false)
        .mode(PipeMode::Bytes)
        .nonblocking(true)
        .security_descriptor(Some(
            SecurityDescriptor::deserialize(&ALLOW_ALL.to_wtf_16().unwrap()).unwrap(),
        ))
        .instance_limit(None)
        .create_duplex::<pipe_mode::Bytes>()
        .unwrap()
}

fn map_ioerr<T>(e: io::Result<T>) -> Result<Option<T>, io::Error> {
    match e {
        Ok(x) => Ok(Some(x)),
        Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
        Err(e) => {
            log::error!("io error: {}", e);
            Err(e)
        }
    }
}

#[derive(Encode, Decode, Debug)]
pub enum IpcMsg {}

#[derive(Default)]
struct VarintState {
    decoded: u64,
    shift: u32,
}

impl VarintState {
    fn read_varint(&mut self, reader: &mut impl Read) -> io::Result<Option<u64>> {
        let mut buf = [0u8; 1];
        loop {
            match reader.read_exact(&mut buf) {
                Ok(()) => {
                    let byte = buf[0];
                    self.decoded |= ((byte & 0x7F) as u64) << self.shift;
                    if byte & 0x80 == 0x80 {
                        self.shift += 7;
                        if self.shift >= 64 {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Varint too large",
                            ));
                        }
                    } else {
                        let result = self.decoded;
                        *self = VarintState::default();
                        return Ok(Some(result));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(None); // No complete varint ready yet
                }
                Err(e) => return Err(e),
            }
        }
    }
}

pub fn write_varint(writer: &mut impl Write, mut value: u64) -> io::Result<()> {
    if value == 0 {
        writer.write_all(&[0])
    } else {
        while value >= 0x80 {
            let byte = ((value & 0x7F) as u8) | 0x80;
            writer.write_all(&[byte])?;
            value >>= 7;
        }
        writer.write_all(&[(value & 0x7F) as u8])
    }
}

enum TransmissionStage {
    Message { buf: Vec<u8>, size: u64 },
    Header(VarintState),
}

struct IpcClient {
    reader: BufReader<RecvPipeStream<Bytes>>,
    writer: BufWriter<SendPipeStream<Bytes>>,
    trans_stage: TransmissionStage,
}

impl IpcClient {
    pub fn new(stream: DuplexPipeStream<Bytes>) -> Self {
        let (r, w) = stream.split();
        IpcClient {
            reader: BufReader::new(r),
            writer: BufWriter::new(w),
            trans_stage: TransmissionStage::Header(VarintState::default()),
        }
    }

    pub fn poll(&mut self) -> io::Result<Option<IpcMsg>> {
        if let TransmissionStage::Header(header) = &mut self.trans_stage {
            if let Some(size) = header.read_varint(&mut self.reader)? {
                self.trans_stage = TransmissionStage::Message {
                    buf: Vec::with_capacity(size as usize),
                    size,
                }
            }

            return Ok(None);
        }

        let TransmissionStage::Message { buf, size } = &mut self.trans_stage else {
            unreachable!()
        };

        let size = *size as usize;
        let pos = buf.len();
        let Some(_) = map_ioerr(self.reader.read(&mut buf[pos..size]))? else {
            return Ok(None);
        };

        if buf.len() == size {
            let (msg, _) =
                match bincode::decode_from_slice::<IpcMsg, _>(&buf, bincode::config::standard()) {
                    Ok(msg) => msg,
                    Err(DecodeError::Io { inner, .. }) => return Err(inner),
                    Err(e) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("encountered invalid data in ipc stream: {}", e),
                        ));
                    }
                };
            self.trans_stage = TransmissionStage::Header(VarintState::default());
            return Ok(Some(msg));
        }

        Ok(None)
    }

    pub fn send(&mut self, msg: IpcMsg) -> io::Result<()> {
        let data = bincode::encode_to_vec(msg, bincode::config::standard()).unwrap();
        write_varint(&mut self.writer, data.len() as u64)?;
        self.writer.write_all(data.as_slice())
    }
}

pub struct PirtIpcServer {
    listener: PipeListener<Bytes, Bytes>,
    clients: HashMap<u32, IpcClient>,
}

impl PirtIpcServer {
    pub fn start(path: &Path) -> Self {
        Self {
            listener: create_listener(path),
            clients: HashMap::new(),
        }
    }

    pub fn is_connected(&self, pid: u32) -> bool {
        unimplemented!()
    }

    pub fn poll(&mut self) -> Vec<IpcMsg> {
        for conn in self.listener.incoming() {
            match conn {
                Ok(stream) => {
                    let id = stream.client_process_id().unwrap();
                    self.clients.insert(id, IpcClient::new(stream));
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                Err(e) => {
                    log::error!("error accepting new ipc connection: {}", e);
                    continue;
                }
            };
        }

        let mut msgs = Vec::new();
        for c in self.clients.values_mut() {
            msgs.push(match c.poll() {
                Err(e) => {
                    log::error!("error in ipc socket: {}", e);
                    continue;
                }
                Ok(None) => continue,
                Ok(Some(msg)) => msg,
            });
        }

        msgs
    }
}
