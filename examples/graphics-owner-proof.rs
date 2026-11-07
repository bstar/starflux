//! A private PTY proves source-side response ordering. No existing terminal,
//! tmux server, desktop window, agent or saved configuration is accessed.
#[path = "../src/graphics_decode.rs"]
mod graphics_decode;
#[path = "../src/graphics_reply.rs"]
mod graphics_reply;
#[path = "../src/graphics_requests.rs"]
mod graphics_requests;
#[path = "../src/graphics_resources.rs"]
mod graphics_resources;
#[path = "../src/graphics_store.rs"]
mod graphics_store;
#[path = "../src/graphics_stream.rs"]
mod graphics_stream;
#[path = "../src/graphics_transfer.rs"]
mod graphics_transfer;
use anyhow::{Context, Result, ensure};
use graphics_requests::{Job, Requests};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};
struct Owner {
    requests: Requests,
    pending: Vec<Job>,
    attributes: usize,
    completed: usize,
}
impl vt100::Callbacks for Owner {
    fn unhandled_apc(&mut self, screen: &mut vt100::Screen, data: &[u8]) {
        match self.requests.push(data, screen.cursor_position()) {
            Ok(Some(job)) => {
                // Deliberately leave the query unresolved until the following DA
                // has been parsed and completed, simulating a slow decoder.
                self.pending.push(job);
            }
            Ok(None) | Err(graphics_requests::Failure::Protocol(_)) => (),
            Err(error) => panic!("private request queue failed: {error:?}"),
        }
    }
    fn cancelled_apc(&mut self, _: &mut vt100::Screen) {
        self.requests.cancel().unwrap();
    }
    fn unhandled_csi(
        &mut self,
        _: &mut vt100::Screen,
        i: Option<u8>,
        j: Option<u8>,
        _: &[&[u16]],
        c: char,
    ) {
        if (i, j, c) == (None, None, 'c') {
            self.requests.ready(b"\x1b[?1;2c".to_vec()).unwrap();
            self.attributes += 1;
        }
    }
}
const SOURCE: &str = r#"import os,select,time,tty
tty.setraw(0)
cases=[
 (b'\x1b_Gi=31,a=q,s=1,v=1,f=24;AAAA\x1b\\',b'\x1b_Gi=31;OK\x1b\\'),
 (b'\x1b_Gi=32,a=q,s=2,v=1,f=24;AAAA\x1b\\',b'\x1b_Gi=32;EINVAL:Invalid graphics data\x1b\\'),
 (b'\x1b_Gi=33,a=q,s=1,v=1,f=24,q=2;AAAA\x1b\\',b''),
 (b'\x1b_Gi=34,a=q,t=f,f=100;L2RvZXMtbm90LWV4aXN0\x1b\\',b'\x1b_Gi=34;ENOTSUP:Unsupported graphics operation\x1b\\'),
 (b'\x1b_Gi=35,a=q,s=1,v=1,f=24,m=1;\x1b\\TEXT\x1b_Gm=0;AAAA\x1b\\',b'\x1b_Gi=35;OK\x1b\\'),
 (b'\x1b_Gi=36,a=q,s=1,v=1,f=24,m=1;\x1b\\\x1b[c\x1b_Gm=0;AAAA\x1b\\',b'\x1b_Gi=36;OK\x1b\\'),
 (b'\x1b_Gi=37,a=q,s=1,v=1,f=24;====\x1b\\',b'\x1b_Gi=37;EINVAL:Invalid graphics data\x1b\\'),
 (b'\x1b_Gi=38,a=q,s=1,v=1,f=24,m=1;\x1b\\\x1b_Gdiscard\x18',b'\x1b_Gi=38;EINVAL:Invalid graphics data\x1b\\'),
]
if os.environ.get('STARFLUX_PROOF_STALL')=='1':
 cases.append((b'\x1b_Gi=39,a=q,s=1,v=1,f=24,m=1;\x1b\\',b'\x1b_Gi=39;ETIMEDOUT:Graphics upload timed out\x1b\\'))
for wrapped in [False,True]:
 for query,reply in cases:
  payload=query+b'\x1b[c'
  if wrapped:payload=b'\x1bPtmux;'+payload.replace(b'\x1b',b'\x1b\x1b')+b'\x1b\\'
  os.write(1,payload)
  expected=reply+b'\x1b[?1;2c'*(2 if b'i=36,' in query else 1);received=b'';end=time.monotonic()+(60 if b'i=39,' in query else 8)
  while len(received)<len(expected):
   remaining=end-time.monotonic()
   if remaining<=0 or not select.select([0],[],[],remaining)[0]:raise SystemExit('source reply timeout')
   received+=os.read(0,4096)
  if received!=expected:raise SystemExit('source replies were duplicated or out of order')
os.write(1,b'\r\nSOURCE_ORDER_OK\r\n')
"#;
fn main() -> Result<()> {
    let stalled = std::env::args().any(|arg| arg == "--stalled-upload");
    let pair = native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 640,
        pixel_height: 384,
    })?;
    let mut command = CommandBuilder::new("python3");
    command.args(["-u", "-c", SOURCE]);
    command.env("STARFLUX_PROOF_STALL", if stalled { "1" } else { "0" });
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let result = (|| -> Result<()> {
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (tx, rx) = crossbeam_channel::bounded(8);
        std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(length) = reader.read(&mut bytes) {
                if length == 0 || tx.send(bytes[..length].to_vec()).is_err() {
                    break;
                }
            }
        });
        let identity = graphics_store::Generation {
            host: "private-owner-proof".into(),
            boot: uuid::Uuid::new_v4(),
            pid: child
                .process_id()
                .context("private child PID unavailable")?,
            start_ticks: 1,
            epoch: uuid::Uuid::new_v4(),
        };
        let resources = graphics_resources::Resources::shared();
        let pool = graphics_decode::Pool::new(resources.clone());
        let owner = Owner {
            requests: Requests::with_resources(identity, resources.clone()),
            pending: Vec::new(),
            attributes: 0,
            completed: 0,
        };
        let mut parser = vt100::Parser::new_with_callbacks(24, 80, 0, owner);
        let mut passthrough = graphics_stream::Passthrough::default();
        let start = Instant::now();
        let mut decoded = Vec::new();
        loop {
            ensure!(
                start.elapsed() < Duration::from_secs(if stalled { 120 } else { 20 }),
                "private owner proof exceeded its budget"
            );
            if let Ok(bytes) = rx.recv_timeout(Duration::from_millis(5)) {
                passthrough.feed(&bytes, |event| match event {
                    graphics_stream::Event::Bytes(data) => parser.process(data),
                    graphics_stream::Event::Gap(gap) => {
                        panic!("unexpected private stream gap: {gap:?}")
                    }
                });
            }
            let owner = parser.callbacks_mut();
            owner.requests.expire(Instant::now()).unwrap();
            for job in owner.pending.drain(..) {
                if let Err(job) = pool.try_submit(0, job) {
                    owner
                        .requests
                        .complete(
                            &job,
                            Err(graphics_reply::Failure::Protocol(
                                graphics_transfer::Error::Busy,
                            )),
                        )
                        .unwrap();
                }
            }
            while let Ok(result) = pool.receive(Duration::ZERO) {
                decoded.push(result);
            }
            if owner.attributes > owner.completed {
                if !decoded.is_empty() {
                    ensure!(
                        owner.requests.drain(8192).is_empty(),
                        "DA escaped before its graphics query completed"
                    );
                }
                for completion in decoded.drain(..).rev() {
                    let result = completion
                        .result
                        .as_ref()
                        .map(|_| ())
                        .map_err(|error| graphics_reply::Failure::Protocol(*error));
                    owner.requests.complete(&completion.job, result).unwrap();
                }
                let bytes = owner.requests.drain(8192);
                if !bytes.is_empty() {
                    writer.write_all(&bytes)?;
                    writer.flush()?;
                    owner.completed = owner.attributes;
                }
            }
            if parser.screen().contents().contains("SOURCE_ORDER_OK") {
                break;
            }
        }
        ensure!(
            parser.callbacks().completed == if stalled { 20 } else { 18 },
            "not every direct/wrapped query was checked"
        );
        ensure!(
            resources.uploads.used() == 0 && resources.working.used() == 0,
            "private owner leaked graphics reservations"
        );
        println!(
            "{}",
            serde_json::json!({"result":"PASS","private_pty":true,"source_side_ordering":true,"direct_cases":if stalled {9}else{8},"wrapped_cases":if stalled {9}else{8},"slow_query_blocks_da":true,"early_chunk_barrier":true,"malformed_and_cancelled":true,"quiet_and_errors":true,"async_decode":true,"reservations_released":true,"real_stalled_upload_expiry":stalled,"no_presenter":true})
        );
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}
