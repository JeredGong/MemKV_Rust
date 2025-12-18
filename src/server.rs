use crate::common::{GetResponse, RemoveResponse, Request, SetResponse};
use crate::engines::{KvsEngine};
use crate::error::{Result};
use crate::thread_pool::{RayonThreadPool, ThreadPool};
use log::{debug, error};
use serde_json::Deserializer;
use std::io::{BufReader, BufWriter, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

/// The server of a key value store.
pub struct KvsServer<E: KvsEngine> {
    engine: E,
}

impl<E: KvsEngine> KvsServer<E> {
    /// Create a `KvsServer` with a given storage engine.
    pub fn new(engine: E) -> Self {
        KvsServer { engine }
    }

    /// Run the server listening on the given address
    pub fn run<A: ToSocketAddrs>(self, addr: A) -> Result<()> {
        let listener = TcpListener::bind(addr)?;
        let threads: u32 = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        let pool = RayonThreadPool::new(threads)?;

        let engine = self.engine;
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let engine = engine.clone();
                    pool.spawn(move || {
                        if let Err(e) = Self::serve(engine, stream) {
                            error!("Error on serving client: {}", e);
                        }
                    });
                }
                Err(e) => error!("Connection failed: {}", e),
            }
        }
        Ok(())
    }

    fn serve(engine: E, tcp: TcpStream) -> Result<()> {
        let peer_addr = tcp.peer_addr()?;
        let reader = BufReader::new(&tcp);
        let mut writer = BufWriter::new(&tcp);
        let req_reader = Deserializer::from_reader(reader).into_iter::<Request>();

        macro_rules! send_resp {
            ($resp:expr) => {{
                let resp = $resp;
                serde_json::to_writer(&mut writer, &resp)?;
                writer.flush()?;
                debug!("Response sent to {}: {:?}", peer_addr, resp);
            }};
        }

        for req in req_reader {
            let req = req?;
            debug!("Receive request from {}: {:?}", peer_addr, req);
            match req {
                Request::Get { key } => send_resp!(match engine.get(key.into()) {
                    Ok(Some(value)) => GetResponse::Ok(Some(String::from_utf8(value.to_vec())?)),
                    Ok(None) => GetResponse::Ok(None),
                    Err(e) => GetResponse::Err(format!("{}", e)),
                }),
                Request::Set { key, value } => send_resp!(match engine.set(key.into(), value.into()) {
                    Ok(_) => SetResponse::Ok(()),
                    Err(e) => SetResponse::Err(format!("{}", e)),
                }),
                Request::Remove { key } => send_resp!(match engine.remove(key.into()) {
                    Ok(_) => RemoveResponse::Ok(()),
                    Err(e) => RemoveResponse::Err(format!("{}", e)),
                }),
            };
        }
        Ok(())
    }
}
