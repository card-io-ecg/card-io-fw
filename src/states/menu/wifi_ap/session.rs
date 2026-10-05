use alloc::rc::Rc;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel, mutex::Mutex};
use macros as cardio;
use network_services::pairing::{perform, Counters, Job, Name, Outcome, SigningKey};

use crate::{
    board::wifi::sta::{Sta, WifiRng},
    task_control::TaskControlToken,
    Shared,
};

/// Held for the length of a scan and of a request, so a request waits for a running scan and
/// a scan does not start during a request.
pub type ScanLock = Mutex<NoopRawMutex, ()>;

pub struct Request {
    pub url: heapless::String<64>,
    pub job: Job,
}

/// Carries the requests of the setup loop to the session task, and the outcomes back.
pub struct Session {
    pub requests: Channel<NoopRawMutex, Request, 1>,
    pub outcomes: Channel<NoopRawMutex, Outcome, 1>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            requests: Channel::new(),
            outcomes: Channel::new(),
        }
    }
}

pub fn new_key() -> SigningKey {
    SigningKey::random(&mut WifiRng::default())
}

#[cardio::task]
pub async fn session_task(
    sta: Sta,
    name: Rc<Name>,
    scan_lock: Rc<ScanLock>,
    counters: Shared<Counters>,
    session: Rc<Session>,
    mut task_control: TaskControlToken<()>,
) {
    info!("Started setup session task");
    task_control
        .run_cancellable(|_| async {
            loop {
                let request = session.requests.receive().await;
                let outcome = run(&sta, &name, &scan_lock, &counters, request).await;
                session.outcomes.send(outcome).await;
            }
        })
        .await;
    info!("Stopped setup session task");
}

async fn run(
    sta: &Sta,
    name: &Name,
    scan_lock: &ScanLock,
    counters: &Shared<Counters>,
    Request { url, job }: Request,
) -> Outcome {
    let _scan = scan_lock.lock().await;

    let Ok(mut client) = sta.client() else {
        warn!("Out of memory for the request client");
        return job.failed();
    };
    perform(
        &mut client.client,
        client.stack,
        &mut WifiRng::default(),
        &url,
        name,
        &mut *counters.lock().await,
        job,
    )
    .await
}
