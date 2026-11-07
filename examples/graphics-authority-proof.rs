//! Disposable raw-PTY acceptance for the source-owner engine. Never opens a
//! renderer, an existing tmux session, a desktop or a real agent.
#[path = "../src/graphics_decode.rs"]
mod graphics_decode;
#[path = "../src/graphics_owner.rs"]
mod graphics_owner;
#[path = "../src/graphics_paint.rs"]
mod graphics_paint;
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
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};
const CHILD: &str = r#"import os,select,time,tty
tty.setraw(0)
da=b'\x1b[?1;2c'
def image(i,extra=b''):
 return b'\x1b_Ga=T,f=24,s=1,v=1,c=2,r=2,i='+str(i).encode()+extra+b';ESIz\x1b\\'
def ok(i):return b'\x1b_Gi='+str(i).encode()+b';OK\x1b\\'
cases=[
 (b'\x1b[2;3H'+image(71)+b'\x1b[cX\x1b[6n',ok(71)+da+b'\x1b[4;6R'),
 (b'\x1b[5;6H'+image(72,b',C=1')+b'\x1b[cY\x1b[6n',ok(72)+da+b'\x1b[5;7R'),
 (b'\x1b[H\x1b_Ga=q,f=24,s=1,v=1,i=73;ESIz\x1b\\\x1b[c\x1b[6n',ok(73)+da+b'\x1b[1;1R'),
 (b'\x1b[H\x1b_Ga=q,t=f,f=100,i=74;L2RvZXMtbm90LWV4aXN0\x1b\\\x1b[c\x1b[6n',b'\x1b_Gi=74;ENOTSUP:Unsupported graphics operation\x1b\\'+da+b'\x1b[1;1R'),
 (b'\x1b[H\x1b_Ga=q,f=24,s=1,v=1,i=75,q=2;ESIz\x1b\\\x1b[c\x1b[6n',da+b'\x1b[1;1R'),
 (b'\x1b[3;3H\x1b_Ga=T,f=24,s=1,v=1,c=2,r=2,C=1,i=76,m=1;\x1b\\\x1b[cab\x1b_Gm=0;ESIz\x1b\\\x1b[cZ\x1b[6n',ok(76)+da+da+b'\x1b[3;6R'),
]
for wrapped in (False,True):
 for command,expected in cases:
  if wrapped:command=b'\x1bPtmux;'+command.replace(b'\x1b',b'\x1b\x1b')+b'\x1b\\'
  os.write(1,command)
  received=b'';end=time.monotonic()+8
  expected=b'U'+expected
  while len(received)<len(expected):
   remaining=end-time.monotonic()
   if remaining<=0 or not select.select([0],[],[],remaining)[0]:raise SystemExit('owner engine response timeout')
   received+=os.read(0,4096)
  if received!=expected:raise SystemExit('owner engine input, cursor, response order or count mismatch: '+repr(received))
if os.environ.get('STARFLUX_PROOF_MUTATIONS')=='1':
 def receive(expected):
  received=b'';end=time.monotonic()+8
  while len(received)<len(expected):
   remaining=end-time.monotonic()
   if remaining<=0 or not select.select([0],[],[],remaining)[0]:raise SystemExit('mutation response timeout')
   received+=os.read(0,4096)
  if received!=expected:raise SystemExit('mutation reply mismatch: '+repr(received))
 os.write(1,b'\x1bc\x1b[2;3H'+image(81,b',C=1')+b'\x1b[24;1H\n\x1b[6n')
 receive(b'U'+ok(81)+b'\x1b[24;1R')
 os.write(1,b'MUTATION_SCROLL_CHECK');receive(b'V')
 os.write(1,b'\x1b[2JMUTATION_CLEAR_CHECK');receive(b'V')
 os.write(1,b'\x1b_Ga=p,i=81,C=1;\x1b\\');receive(ok(81))
 os.write(1,b'\x1bcMUTATION_RESET_CHECK');receive(b'V')
os.write(1,b'\r\nSOURCE_OWNER_OK\r\n')
"#;
fn main() -> Result<()> {
    let mutations = std::env::args().any(|a| a == "--mutations");
    let pair = native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 640,
        pixel_height: 384,
    })?;
    let mut command = CommandBuilder::new("python3");
    command.args(["-u", "-c", CHILD]);
    command.env(
        "STARFLUX_PROOF_MUTATIONS",
        if mutations { "1" } else { "0" },
    );
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let result = (|| -> Result<()> {
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (tx, rx) = crossbeam_channel::bounded(8);
        std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(n) = reader.read(&mut bytes) {
                if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let identity = graphics_store::Generation {
            host: "private-engine-pty".into(),
            boot: uuid::Uuid::new_v4(),
            pid: child.process_id().context("No private child PID")?,
            start_ticks: 1,
            epoch: uuid::Uuid::new_v4(),
        };
        let resources = graphics_resources::Resources::new(64 * 1024 * 1024, 512 * 1024 * 1024);
        let mut owner = graphics_owner::Owner::new(
            identity,
            graphics_store::Geometry {
                rows: 24,
                columns: 80,
                cell: [8, 16],
            },
            resources.clone(),
        )
        .map_err(|e| anyhow::anyhow!("Owner startup: {e:?}"))?;
        let pool = graphics_decode::Pool::new(resources.clone());
        let start = Instant::now();
        let mut input_during_decode = 0;
        let mut waiting = None;
        let mut mutation_stage = 0;
        loop {
            ensure!(
                start.elapsed() < Duration::from_secs(20),
                "Private engine PTY proof timed out"
            );
            if waiting.is_none() {
                waiting = rx.recv_timeout(Duration::from_millis(2)).ok();
            }
            if let Some(bytes) = &waiting
                && owner
                    .ingest(bytes)
                    .map_err(|e| anyhow::anyhow!("Ingest: {e:?}"))?
            {
                waiting = None;
            }
            owner
                .pump(8192, Instant::now())
                .map_err(|e| anyhow::anyhow!("Pump: {e:?}"))?;
            if let Some((tag, job)) = owner.take_job() {
                ensure!(
                    owner.pending(),
                    "Decode barrier disappeared before user input"
                );
                writer.write_all(b"U")?;
                writer.flush()?;
                input_during_decode += 1;
                if let Err(job) = pool.try_submit(tag, job) {
                    owner
                        .complete(graphics_decode::Completion {
                            tag,
                            job: *job,
                            result: Err(graphics_transfer::Error::Busy),
                        })
                        .map_err(|e| anyhow::anyhow!("Busy completion: {e:?}"))?;
                }
            }
            while let Ok(completion) = pool.receive(Duration::ZERO) {
                owner
                    .complete(completion)
                    .map_err(|e| anyhow::anyhow!("Completion: {e:?}"))?;
            }
            let replies = owner.drain_replies(8192);
            if !replies.is_empty() {
                writer.write_all(&replies)?;
                writer.flush()?;
            }
            let text = owner.screen().contents();
            if mutations && mutation_stage == 0 && text.contains("MUTATION_SCROLL_CHECK") {
                let p = owner
                    .store()
                    .placements(false)
                    .next()
                    .context("Scrolled image missing")?;
                ensure!(p.row() == 0, "Image did not follow real PTY linefeed");
                let raster = graphics_paint::compose(
                    owner.store(),
                    graphics_paint::View {
                        origin: [0, 0],
                        width: 640,
                        height: 384,
                        cell: [8, 16],
                        alternate: false,
                    },
                    &[],
                    &resources,
                )
                .map_err(|e| anyhow::anyhow!("Scrolled raster: {e:?}"))?;
                ensure!(
                    raster.sample(graphics_paint::Plane::AboveText, 16, 8) == [17, 34, 51, 255],
                    "Real PTY scroll changed visible image pixels"
                );
                writer.write_all(b"V")?;
                writer.flush()?;
                mutation_stage = 1;
            }
            if mutations && mutation_stage == 1 && text.contains("MUTATION_CLEAR_CHECK") {
                ensure!(
                    owner.store().placements(false).count() == 0 && resources.retained.used() == 4,
                    "Display clear lost reusable image data or retained a visible placement"
                );
                writer.write_all(b"V")?;
                writer.flush()?;
                mutation_stage = 2;
            }
            if mutations && mutation_stage == 2 && text.contains("MUTATION_RESET_CHECK") {
                ensure!(
                    owner.store().placements(false).count() == 0 && resources.retained.used() == 0,
                    "Terminal reset retained image data"
                );
                writer.write_all(b"V")?;
                writer.flush()?;
                mutation_stage = 3;
            }
            if owner.screen().contents().contains("SOURCE_OWNER_OK") {
                break;
            }
        }
        ensure!(
            input_during_decode == if mutations { 13 } else { 12 },
            "Not every query passed the live input/decode barrier"
        );
        ensure!(
            owner.store().placements(false).count() == if mutations { 0 } else { 3 },
            "Unexpected retained placement count"
        );
        let retained = resources.retained.used();
        ensure!(
            retained == if mutations { 0 } else { 12 },
            "Retained image capacity was not charged"
        );
        ensure!(
            !mutations || mutation_stage == 3,
            "Not every real PTY mutation was checked"
        );
        drop(owner);
        ensure!(
            resources.uploads.used() == 0
                && resources.working.used() == 0
                && resources.retained.used() == 0,
            "Owner engine reservations leaked"
        );
        println!(
            "{}",
            serde_json::json!({"result":"PASS","private_pty":true,"source_owner_engine":true,"direct_cases":6,"wrapped_cases":6,"commit_barrier":true,"cursor_and_text_order":true,"ordered_graphics_da_dsr":true,"input_while_decode_pending":true,"mutation_checks":mutations,"retained_images":if mutations {0}else{3},"retained_bytes":retained,"released_reservations":true,"no_presenter":true})
        );
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}
