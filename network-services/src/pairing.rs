use alloc::{
    format,
    rc::Rc,
    string::{String, ToString},
};
use core::{
    fmt::{self, Write},
    mem,
};

pub use device_auth_firmware::{parse_code, Code, Counter, SigningKey};
use device_auth_firmware::{registration, request_token};
use edge_http::Method;
use embassy_net::Stack;
use embassy_time::{with_timeout, Duration};
use rand_core::CryptoRngCore;

use crate::{
    client::Client,
    http::{Body, Request},
    url::{self, BaseUrl},
};

const DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Name(heapless::String<19>);

impl Name {
    pub fn from_mac(mac: [u8; 6]) -> Self {
        let mut name = heapless::String::new();
        unwrap!(name.push_str("cardio-"));
        for byte in mac {
            unwrap!(write!(name, "{byte:02x}"));
        }
        Self(name)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Copy)]
pub enum Template {
    Unpair,
}

#[derive(Default)]
pub struct Counters {
    unpair: Counter,
}

impl Counters {
    pub fn get(&mut self, template: Template) -> &mut Counter {
        match template {
            Template::Unpair => &mut self.unpair,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Failure {
    Rejected,
    Taken,
    Failed,
}

impl Failure {
    fn as_str(self) -> &'static str {
        match self {
            Failure::Rejected => "rejected",
            Failure::Taken => "taken",
            Failure::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct UnpairFailed;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Status {
    Unpaired(Option<Failure>),
    Paired(Option<UnpairFailed>),
    Pairing,
    Unpairing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Refusal {
    Code,
    Wifi,
    Busy,
    Save,
    Url,
    Paired,
    Unpaired,
}

impl Refusal {
    pub fn as_str(&self) -> &'static str {
        match self {
            Refusal::Code => "code",
            Refusal::Wifi => "wifi",
            Refusal::Busy => "busy",
            Refusal::Save => "save",
            Refusal::Url => "url",
            Refusal::Paired => "paired",
            Refusal::Unpaired => "unpaired",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PairAnswer {
    Registered(SigningKey),
    Failed(Failure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum UnpairAnswer {
    Removed,
    Refused,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum UnpairStep {
    Done(UnpairAnswer),
    Resync(u64),
}

pub fn pair_answer(status: u16, key: SigningKey) -> PairAnswer {
    match status {
        201 => PairAnswer::Registered(key),
        401 => PairAnswer::Failed(Failure::Rejected),
        409 => PairAnswer::Failed(Failure::Taken),
        _ => PairAnswer::Failed(Failure::Failed),
    }
}

/// `retried` is true for the second attempt.
pub fn unpair_step(status: u16, counter: Option<u64>, retried: bool) -> UnpairStep {
    match (status, counter) {
        (204, _) => UnpairStep::Done(UnpairAnswer::Removed),
        (401, Some(last)) if !retried => UnpairStep::Resync(last),
        (401, None) => UnpairStep::Done(UnpairAnswer::Refused),
        _ => UnpairStep::Done(UnpairAnswer::Failed),
    }
}

enum State {
    Unpaired(Option<Failure>),
    Paired(Rc<SigningKey>, Option<UnpairFailed>),
    Refused(Rc<SigningKey>, Option<Failure>),
    /// The key from before, which stays until a `201` replaces it.
    Pairing(Option<Rc<SigningKey>>),
    Unpairing(Rc<SigningKey>),
}

pub struct Pairing {
    state: State,
}

impl Pairing {
    pub fn new(key: Option<SigningKey>) -> Self {
        let state = match key {
            Some(key) => State::Paired(Rc::new(key), None),
            None => State::Unpaired(None),
        };
        Self { state }
    }

    pub fn paired(&self) -> bool {
        matches!(self.state, State::Paired(..))
    }

    pub fn status(&self) -> Status {
        match &self.state {
            State::Unpaired(result) | State::Refused(_, result) => Status::Unpaired(*result),
            State::Paired(_, failed) => Status::Paired(*failed),
            State::Pairing(_) => Status::Pairing,
            State::Unpairing(_) => Status::Unpairing,
        }
    }

    pub fn start_pair(&mut self) -> Result<(), Refusal> {
        let previous = match &self.state {
            State::Unpaired(_) => None,
            State::Refused(key, _) => Some(key.clone()),
            State::Paired(..) => return Err(Refusal::Paired),
            State::Pairing(_) | State::Unpairing(_) => return Err(Refusal::Busy),
        };
        self.state = State::Pairing(previous);
        Ok(())
    }

    pub fn pair_answered(&mut self, answer: PairAnswer) {
        self.transition(|state| match (state, answer) {
            (State::Pairing(_), PairAnswer::Registered(key)) => State::Paired(Rc::new(key), None),
            // The server knows this name, so the old key may be the one it holds.
            (State::Pairing(Some(key)), PairAnswer::Failed(Failure::Taken)) => {
                State::Paired(key, None)
            }
            (State::Pairing(Some(key)), PairAnswer::Failed(failure)) => {
                State::Refused(key, Some(failure))
            }
            (State::Pairing(None), PairAnswer::Failed(failure)) => State::Unpaired(Some(failure)),
            (state, _) => state,
        });
    }

    pub fn start_unpair(&mut self) -> Result<Rc<SigningKey>, Refusal> {
        match &self.state {
            State::Paired(key, _) => {
                let key = key.clone();
                self.state = State::Unpairing(key.clone());
                Ok(key)
            }
            State::Unpaired(_) | State::Refused(..) => Err(Refusal::Unpaired),
            State::Pairing(_) | State::Unpairing(_) => Err(Refusal::Busy),
        }
    }

    pub fn unpair_answered(&mut self, answer: UnpairAnswer) {
        self.transition(|state| match (state, answer) {
            (State::Unpairing(_), UnpairAnswer::Removed) => State::Unpaired(None),
            (State::Unpairing(key), UnpairAnswer::Refused) => {
                State::Refused(key, Some(Failure::Failed))
            }
            (State::Unpairing(key), UnpairAnswer::Failed) => State::Paired(key, Some(UnpairFailed)),
            (state, _) => state,
        });
    }

    pub fn saved(&mut self) {
        self.transition(|state| match state {
            State::Refused(key, _) => State::Paired(key, None),
            state => state,
        });
    }

    pub fn session_opened(&mut self) {
        match &mut self.state {
            State::Unpaired(result) | State::Refused(_, result) => *result = None,
            State::Paired(_, failed) => *failed = None,
            State::Pairing(_) | State::Unpairing(_) => {}
        }
    }

    fn transition(&mut self, next: impl FnOnce(State) -> State) {
        let state = mem::replace(&mut self.state, State::Unpaired(None));
        self.state = next(state);
    }
}

pub fn format(name: &Name, status: &Status, out: &mut impl Write) -> fmt::Result {
    let (state, result) = match status {
        Status::Unpaired(result) => ("unpaired", result.map(Failure::as_str)),
        Status::Paired(failed) => ("paired", failed.map(|UnpairFailed| "failed")),
        Status::Pairing => ("pairing", None),
        Status::Unpairing => ("unpairing", None),
    };
    write!(out, "{} {state}", name.as_str())?;
    match result {
        Some(result) => write!(out, " {result}"),
        None => Ok(()),
    }
}

pub enum Job {
    Register(Code, SigningKey),
    Unpair(Rc<SigningKey>),
}

impl Job {
    /// The outcome of a job that could not run to an answer.
    pub fn failed(&self) -> Outcome {
        match self {
            Job::Register(..) => Outcome::Registered(PairAnswer::Failed(Failure::Failed)),
            Job::Unpair(_) => Outcome::Unpaired(UnpairAnswer::Failed),
        }
    }
}

pub enum Outcome {
    Registered(PairAnswer),
    Unpaired(UnpairAnswer),
}

struct Link<'a> {
    client: &'a mut Client,
    stack: Stack<'a>,
    rng: &'a mut dyn CryptoRngCore,
    base: BaseUrl<'a>,
}

struct Answer {
    status: u16,
    counter: Option<u64>,
}

impl Link<'_> {
    // The answer is decided by its status and counter header. The body stays unread, and
    // dropping the connection discards it.
    async fn exchange(&mut self, request: &Request<'_>) -> Option<Answer> {
        let mut connection = self
            .client
            .connect(self.stack, &mut *self.rng, &self.base)
            .await
            .ok()?;
        let response = connection.send(request).await.ok()?;
        Some(Answer {
            status: response.status,
            counter: response.counter,
        })
    }
}

/// Runs one job against `base_url` within the 30-second deadline, signing included.
pub async fn perform(
    client: &mut Client,
    stack: Stack<'_>,
    rng: &mut dyn CryptoRngCore,
    base_url: &str,
    name: &Name,
    counters: &mut Counters,
    job: Job,
) -> Outcome {
    let Some(base) = url::parse(base_url) else {
        warn!("The backend URL does not parse");
        return job.failed();
    };
    let mut link = Link {
        client,
        stack,
        rng,
        base,
    };
    match job {
        Job::Register(code, key) => {
            Outcome::Registered(register(&mut link, name, &code, key).await)
        }
        Job::Unpair(key) => Outcome::Unpaired(unpair(&mut link, name, counters, &key).await),
    }
}

async fn register(link: &mut Link<'_>, name: &Name, code: &Code, key: SigningKey) -> PairAnswer {
    let answered = with_timeout(DEADLINE, async {
        let body = registration_body(&key, name, code);
        let path = registration_path(link.base.path);
        link.exchange(&Request {
            method: Method::Post,
            path: &path,
            authorization: None,
            body: Some(Body {
                content_type: "application/jwt",
                parts: &[body.as_bytes()],
            }),
        })
        .await
    })
    .await;

    let Ok(Some(Answer { status, .. })) = answered else {
        warn!("The registration did not finish");
        return PairAnswer::Failed(Failure::Failed);
    };
    info!("The registration answered with status {}", status);
    pair_answer(status, key)
}

async fn unpair(
    link: &mut Link<'_>,
    name: &Name,
    counters: &mut Counters,
    key: &SigningKey,
) -> UnpairAnswer {
    let answered = with_timeout(DEADLINE, async {
        let path = unpair_path(link.base.path, name);
        let mut retried = false;
        loop {
            let jti = counters.get(Template::Unpair).next_jti();
            let authorization = bearer(key, name, jti, Method::Delete, &path);
            let request = Request {
                method: Method::Delete,
                path: &path,
                authorization: Some(&authorization),
                body: None,
            };
            let Some(Answer { status, counter }) = link.exchange(&request).await else {
                return UnpairAnswer::Failed;
            };
            info!("The unpair answered with status {}", status);
            match unpair_step(status, counter, retried) {
                UnpairStep::Done(answer) => return answer,
                UnpairStep::Resync(last) => {
                    counters.get(Template::Unpair).resync(last);
                    retried = true;
                }
            }
        }
    })
    .await;

    answered.unwrap_or_else(|_| {
        warn!("The unpair did not finish");
        UnpairAnswer::Failed
    })
}

fn registration_path(base_path: &str) -> String {
    format!("{base_path}/devices")
}

fn unpair_path(base_path: &str, name: &Name) -> String {
    format!("{base_path}/devices/{}", name.as_str())
}

fn registration_body(key: &SigningKey, name: &Name, code: &Code) -> String {
    let mut body = String::new();
    unwrap!(registration(key, name.as_str(), code, &mut body));
    body
}

fn bearer(key: &SigningKey, name: &Name, jti: u64, method: Method, path: &str) -> String {
    let mut authorization = String::from("Bearer ");
    unwrap!(request_token(
        key,
        name.as_str(),
        jti,
        &method.to_string(),
        path,
        &mut authorization
    ));
    authorization
}

#[cfg(test)]
mod test {
    use std::{format, string::String};

    use device_auth_firmware::{parse_code, registration, request_token, valid_name};

    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32].into()).unwrap()
    }

    fn unpaired() -> Pairing {
        Pairing::new(None)
    }

    fn paired() -> Pairing {
        Pairing::new(Some(key(1)))
    }

    fn unpairing() -> Pairing {
        let mut pairing = paired();
        pairing.start_unpair().unwrap();
        pairing
    }

    fn refused() -> Pairing {
        let mut pairing = unpairing();
        pairing.unpair_answered(UnpairAnswer::Refused);
        pairing
    }

    fn pairing_from_unpaired() -> Pairing {
        let mut pairing = unpaired();
        pairing.start_pair().unwrap();
        pairing
    }

    fn pairing_from_refused() -> Pairing {
        let mut pairing = refused();
        pairing.start_pair().unwrap();
        pairing
    }

    fn unpaired_with(failure: Failure) -> Pairing {
        let mut pairing = pairing_from_unpaired();
        pairing.pair_answered(PairAnswer::Failed(failure));
        pairing
    }

    fn paired_with_unpair_failed() -> Pairing {
        let mut pairing = unpairing();
        pairing.unpair_answered(UnpairAnswer::Failed);
        pairing
    }

    /// The key a settled `Pairing` holds. A save moves Refused to Paired, where Unpair hands the
    /// key out.
    fn held_key(mut pairing: Pairing) -> Option<SigningKey> {
        pairing.saved();
        pairing.start_unpair().ok().map(|key| (*key).clone())
    }

    const ALL_STATES: [fn() -> Pairing; 5] =
        [unpaired, paired, refused, pairing_from_unpaired, unpairing];

    fn name() -> Name {
        Name::from_mac([0x68, 0xb6, 0xb3, 0x2d, 0x94, 0x7c])
    }

    #[test]
    fn name_is_cardio_and_the_mac_as_twelve_lowercase_hex_digits() {
        assert_eq!(name().as_str(), "cardio-68b6b32d947c");
    }

    #[test]
    fn name_pads_mac_bytes_below_0x10_to_two_digits() {
        let name = Name::from_mac([0x00, 0x01, 0x0a, 0x0f, 0x10, 0xff]);
        assert_eq!(name.as_str(), "cardio-00010a0f10ff");
    }

    #[test]
    fn every_name_is_valid_for_device_auth() {
        for mac in [[0x00; 6], [0xff; 6], [0x01, 0x02, 0x03, 0x04, 0x05, 0x06]] {
            let name = Name::from_mac(mac);
            assert_eq!(name.as_str().len(), 19);
            assert!(valid_name(name.as_str()), "{}", name.as_str());
        }
    }

    #[test]
    fn the_unpair_counter_gives_1_first_and_counts_up() {
        let mut counters = Counters::default();
        assert_eq!(counters.get(Template::Unpair).next_jti(), 1);
        assert_eq!(counters.get(Template::Unpair).next_jti(), 2);
    }

    #[test]
    fn the_unpair_counter_continues_after_the_resynced_value() {
        let mut counters = Counters::default();
        counters.get(Template::Unpair).next_jti();
        counters.get(Template::Unpair).resync(41);
        assert_eq!(counters.get(Template::Unpair).next_jti(), 42);
    }

    #[test]
    fn refusals_have_their_400_words() {
        let words = [
            (Refusal::Code, "code"),
            (Refusal::Wifi, "wifi"),
            (Refusal::Busy, "busy"),
            (Refusal::Save, "save"),
            (Refusal::Url, "url"),
            (Refusal::Paired, "paired"),
            (Refusal::Unpaired, "unpaired"),
        ];
        for (refusal, word) in words {
            assert_eq!(refusal.as_str(), word);
        }
    }

    #[test]
    fn registration_201_registers_the_key() {
        assert_eq!(pair_answer(201, key(2)), PairAnswer::Registered(key(2)));
    }

    #[test]
    fn registration_401_rejects_the_code() {
        assert_eq!(
            pair_answer(401, key(2)),
            PairAnswer::Failed(Failure::Rejected)
        );
    }

    #[test]
    fn registration_409_reports_a_taken_name() {
        assert_eq!(pair_answer(409, key(2)), PairAnswer::Failed(Failure::Taken));
    }

    #[test]
    fn registration_with_any_other_status_fails() {
        for status in [0, 100, 200, 204, 304, 400, 403, 404, 500, 503] {
            assert_eq!(
                pair_answer(status, key(2)),
                PairAnswer::Failed(Failure::Failed),
                "{status}"
            );
        }
    }

    #[test]
    fn unpair_204_removes_the_device() {
        for counter in [None, Some(7)] {
            assert_eq!(
                unpair_step(204, counter, false),
                UnpairStep::Done(UnpairAnswer::Removed)
            );
        }
    }

    #[test]
    fn unpair_401_with_a_counter_resyncs_once() {
        assert_eq!(unpair_step(401, Some(41), false), UnpairStep::Resync(41));
        assert_eq!(unpair_step(401, Some(0), false), UnpairStep::Resync(0));
    }

    #[test]
    fn unpair_401_with_a_counter_after_a_resync_fails() {
        assert_eq!(
            unpair_step(401, Some(41), true),
            UnpairStep::Done(UnpairAnswer::Failed)
        );
    }

    #[test]
    fn unpair_401_without_a_counter_is_refused_on_either_attempt() {
        for retried in [false, true] {
            assert_eq!(
                unpair_step(401, None, retried),
                UnpairStep::Done(UnpairAnswer::Refused)
            );
        }
    }

    #[test]
    fn unpair_with_any_other_status_fails() {
        for status in [0, 100, 200, 304, 400, 403, 404, 409, 500] {
            for counter in [None, Some(7)] {
                for retried in [false, true] {
                    assert_eq!(
                        unpair_step(status, counter, retried),
                        UnpairStep::Done(UnpairAnswer::Failed),
                        "{status} {counter:?} {retried}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_loaded_key_starts_paired() {
        let pairing = paired();
        assert_eq!(pairing.status(), Status::Paired(None));
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn no_key_starts_unpaired() {
        let pairing = unpaired();
        assert_eq!(pairing.status(), Status::Unpaired(None));
        assert_eq!(held_key(pairing), None);
    }

    #[test]
    fn only_the_paired_state_is_paired() {
        let expected = [false, true, false, false, false];
        for (state, expected) in ALL_STATES.iter().zip(expected) {
            assert_eq!(state().paired(), expected, "{:?}", state().status());
        }
    }

    #[test]
    fn pair_from_unpaired_starts_pairing() {
        let mut pairing = unpaired();
        assert_eq!(pairing.start_pair(), Ok(()));
        assert_eq!(pairing.status(), Status::Pairing);
    }

    #[test]
    fn pair_from_refused_starts_pairing() {
        let mut pairing = refused();
        assert_eq!(pairing.start_pair(), Ok(()));
        assert_eq!(pairing.status(), Status::Pairing);
    }

    #[test]
    fn pair_from_paired_is_refused_with_paired() {
        let mut pairing = paired();
        assert_eq!(pairing.start_pair(), Err(Refusal::Paired));
        assert_eq!(pairing.status(), Status::Paired(None));
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn pair_while_pairing_is_refused_with_busy() {
        let mut pairing = pairing_from_unpaired();
        assert_eq!(pairing.start_pair(), Err(Refusal::Busy));
        assert_eq!(pairing.status(), Status::Pairing);
    }

    #[test]
    fn pair_while_unpairing_is_refused_with_busy() {
        let mut pairing = unpairing();
        assert_eq!(pairing.start_pair(), Err(Refusal::Busy));
        assert_eq!(pairing.status(), Status::Unpairing);
    }

    #[test]
    fn a_refused_pair_keeps_the_result() {
        let mut pairing = paired_with_unpair_failed();
        assert_eq!(pairing.start_pair(), Err(Refusal::Paired));
        assert_eq!(pairing.status(), Status::Paired(Some(UnpairFailed)));
    }

    #[test]
    fn registration_201_from_unpaired_pairs_with_the_new_key() {
        let mut pairing = pairing_from_unpaired();
        pairing.pair_answered(PairAnswer::Registered(key(2)));
        assert_eq!(pairing.status(), Status::Paired(None));
        assert_eq!(held_key(pairing), Some(key(2)));
    }

    #[test]
    fn registration_201_from_refused_replaces_the_old_key_and_clears_the_result() {
        let mut pairing = pairing_from_refused();
        pairing.pair_answered(PairAnswer::Registered(key(2)));
        assert_eq!(pairing.status(), Status::Paired(None));
        assert_eq!(held_key(pairing), Some(key(2)));
    }

    #[test]
    fn pair_from_refused_keeps_the_old_key_until_the_answer() {
        let mut pairing = pairing_from_refused();
        pairing.pair_answered(PairAnswer::Failed(Failure::Failed));
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn registration_409_from_refused_returns_to_paired_with_the_old_key() {
        let mut pairing = pairing_from_refused();
        pairing.pair_answered(PairAnswer::Failed(Failure::Taken));
        assert_eq!(pairing.status(), Status::Paired(None));
        assert!(pairing.paired());
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn registration_failure_from_unpaired_returns_to_unpaired_with_the_result() {
        for failure in [Failure::Rejected, Failure::Taken, Failure::Failed] {
            let pairing = unpaired_with(failure);
            assert_eq!(pairing.status(), Status::Unpaired(Some(failure)));
            assert_eq!(held_key(pairing), None);
        }
    }

    #[test]
    fn registration_failure_from_refused_returns_to_refused_with_the_result() {
        for failure in [Failure::Rejected, Failure::Failed] {
            let mut pairing = pairing_from_refused();
            pairing.pair_answered(PairAnswer::Failed(failure));
            assert_eq!(pairing.status(), Status::Unpaired(Some(failure)));
            assert!(!pairing.paired());
            assert_eq!(held_key(pairing), Some(key(1)));
        }
    }

    #[test]
    fn a_new_pair_replaces_the_earlier_result() {
        let mut pairing = unpaired_with(Failure::Rejected);
        pairing.start_pair().unwrap();
        assert_eq!(pairing.status(), Status::Pairing);
        pairing.pair_answered(PairAnswer::Failed(Failure::Taken));
        assert_eq!(pairing.status(), Status::Unpaired(Some(Failure::Taken)));
    }

    #[test]
    fn unpair_from_paired_starts_unpairing_and_hands_out_the_key() {
        let mut pairing = paired();
        assert_eq!(pairing.start_unpair(), Ok(Rc::new(key(1))));
        assert_eq!(pairing.status(), Status::Unpairing);
        assert!(!pairing.paired());
    }

    #[test]
    fn unpair_from_unpaired_is_refused_with_unpaired() {
        let mut pairing = unpaired();
        assert_eq!(pairing.start_unpair(), Err(Refusal::Unpaired));
        assert_eq!(pairing.status(), Status::Unpaired(None));
    }

    #[test]
    fn unpair_from_refused_is_refused_with_unpaired() {
        let mut pairing = refused();
        assert_eq!(pairing.start_unpair(), Err(Refusal::Unpaired));
        assert_eq!(pairing.status(), Status::Unpaired(Some(Failure::Failed)));
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn unpair_while_pairing_is_refused_with_busy() {
        let mut pairing = pairing_from_unpaired();
        assert_eq!(pairing.start_unpair(), Err(Refusal::Busy));
        assert_eq!(pairing.status(), Status::Pairing);
    }

    #[test]
    fn unpair_while_unpairing_is_refused_with_busy() {
        let mut pairing = unpairing();
        assert_eq!(pairing.start_unpair(), Err(Refusal::Busy));
        assert_eq!(pairing.status(), Status::Unpairing);
    }

    #[test]
    fn a_refused_unpair_keeps_the_result() {
        let mut pairing = unpaired_with(Failure::Rejected);
        assert_eq!(pairing.start_unpair(), Err(Refusal::Unpaired));
        assert_eq!(pairing.status(), Status::Unpaired(Some(Failure::Rejected)));
    }

    #[test]
    fn unpair_204_moves_to_unpaired_and_forgets_the_key() {
        let mut pairing = unpairing();
        pairing.unpair_answered(UnpairAnswer::Removed);
        assert_eq!(pairing.status(), Status::Unpaired(None));
        assert_eq!(held_key(pairing), None);
    }

    #[test]
    fn unpair_401_without_a_counter_moves_to_refused_with_the_result_failed() {
        let mut pairing = unpairing();
        pairing.unpair_answered(UnpairAnswer::Refused);
        assert_eq!(pairing.status(), Status::Unpaired(Some(Failure::Failed)));
        assert!(!pairing.paired());
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn a_failed_unpair_returns_to_paired_with_the_result_failed() {
        let pairing = paired_with_unpair_failed();
        assert_eq!(pairing.status(), Status::Paired(Some(UnpairFailed)));
        assert!(pairing.paired());
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn a_new_unpair_replaces_the_earlier_result() {
        let mut pairing = paired_with_unpair_failed();
        pairing.start_unpair().unwrap();
        assert_eq!(pairing.status(), Status::Unpairing);
        pairing.unpair_answered(UnpairAnswer::Removed);
        assert_eq!(pairing.status(), Status::Unpaired(None));
    }

    #[test]
    fn opening_setup_clears_the_result_of_an_unpaired_device() {
        let mut pairing = unpaired_with(Failure::Rejected);
        pairing.session_opened();
        assert_eq!(pairing.status(), Status::Unpaired(None));
    }

    #[test]
    fn opening_setup_clears_the_result_of_a_paired_device() {
        let mut pairing = paired_with_unpair_failed();
        pairing.session_opened();
        assert_eq!(pairing.status(), Status::Paired(None));
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn opening_setup_clears_the_result_of_a_refused_device_and_keeps_its_key() {
        let mut pairing = refused();
        pairing.session_opened();
        assert_eq!(pairing.status(), Status::Unpaired(None));
        assert!(!pairing.paired());
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn opening_setup_does_not_touch_a_running_request() {
        let mut pairing = pairing_from_unpaired();
        pairing.session_opened();
        assert_eq!(pairing.status(), Status::Pairing);

        let mut pairing = unpairing();
        pairing.session_opened();
        assert_eq!(pairing.status(), Status::Unpairing);
    }

    #[test]
    fn a_save_moves_refused_to_paired_with_the_same_key_and_no_result() {
        let mut pairing = refused();
        pairing.saved();
        assert_eq!(pairing.status(), Status::Paired(None));
        assert!(pairing.paired());
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn a_save_changes_nothing_when_unpaired() {
        let mut pairing = unpaired_with(Failure::Rejected);
        pairing.saved();
        assert_eq!(pairing.status(), Status::Unpaired(Some(Failure::Rejected)));
    }

    #[test]
    fn a_save_changes_nothing_when_paired() {
        let mut pairing = paired_with_unpair_failed();
        pairing.saved();
        assert_eq!(pairing.status(), Status::Paired(Some(UnpairFailed)));
    }

    #[test]
    fn a_save_changes_nothing_during_a_request() {
        let mut pairing = pairing_from_refused();
        pairing.saved();
        assert_eq!(pairing.status(), Status::Pairing);
        pairing.pair_answered(PairAnswer::Failed(Failure::Failed));
        assert_eq!(pairing.status(), Status::Unpaired(Some(Failure::Failed)));

        let mut pairing = unpairing();
        pairing.saved();
        assert_eq!(pairing.status(), Status::Unpairing);
    }

    #[test]
    fn a_registration_answer_without_a_registration_changes_nothing() {
        for state in [unpaired, paired, refused, unpairing] {
            for answer in [
                PairAnswer::Registered(key(2)),
                PairAnswer::Failed(Failure::Taken),
                PairAnswer::Failed(Failure::Rejected),
            ] {
                let mut pairing = state();
                pairing.pair_answered(answer);
                assert_eq!(pairing.status(), state().status());
                assert_eq!(pairing.paired(), state().paired());
            }
        }
        let mut pairing = paired();
        pairing.pair_answered(PairAnswer::Registered(key(2)));
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn an_unpair_answer_without_an_unpair_changes_nothing() {
        let answers = [
            UnpairAnswer::Removed,
            UnpairAnswer::Refused,
            UnpairAnswer::Failed,
        ];
        for state in [unpaired, paired, refused, pairing_from_unpaired] {
            for answer in answers {
                let mut pairing = state();
                pairing.unpair_answered(answer);
                assert_eq!(pairing.status(), state().status());
                assert_eq!(pairing.paired(), state().paired());
            }
        }
        let mut pairing = paired();
        pairing.unpair_answered(UnpairAnswer::Removed);
        assert_eq!(held_key(pairing), Some(key(1)));
    }

    #[test]
    fn format_writes_the_name_the_status_and_the_result() {
        let rows = [
            (Status::Unpaired(None), "unpaired"),
            (
                Status::Unpaired(Some(Failure::Rejected)),
                "unpaired rejected",
            ),
            (Status::Unpaired(Some(Failure::Taken)), "unpaired taken"),
            (Status::Unpaired(Some(Failure::Failed)), "unpaired failed"),
            (Status::Paired(None), "paired"),
            (Status::Paired(Some(UnpairFailed)), "paired failed"),
            (Status::Pairing, "pairing"),
            (Status::Unpairing, "unpairing"),
        ];
        for (status, expected) in rows {
            let mut line = String::new();
            format(&name(), &status, &mut line).unwrap();
            assert_eq!(line, format!("cardio-68b6b32d947c {expected}"));
        }
    }

    // The signing test on the chip expects the same values. Change both together.
    const NEUTRAL_NAME_MAC: [u8; 6] = [0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6];
    const NEUTRAL_CODE: &str = "7KQ2MX9D4F";
    const NEUTRAL_REGISTRATION: &str = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImp3ayI6eyJrdHkiOiJFQyIsImNydiI6IlAtMjU2IiwieCI6ImJfQTdsSkpCemgydDFEVVo1cFlPQ29XMEdtbWdYREtCQTZvcnpoV1V5aFkiLCJ5IjoiUEU5MU9sV19BZHhUOXNDd3gtN25pMERHXzMwbHFXNGlncm1KenZjY0ZFbyJ9fQ.eyJuYW1lIjoiY2FyZGlvLWExYjJjM2Q0ZTVmNiIsImNvZGVfbWFjIjoiNEItbG5GU0xGbDJTNmNXV0J4SWRKZmFlNVFlVEE2UHVncHZBM0xTc3cyNCJ9.zBs3SmYaY6Tm-UNyIbsz37r74B648gG4ZMDXPj4Pz-4lXajle9vvMa9TtD6uxztsRvvt_tvfyGia1QjVx-EIjg";
    const NEUTRAL_TOKEN: &str = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJjYXJkaW8tYTFiMmMzZDRlNWY2IiwianRpIjoiNDIiLCJodG0iOiJERUxFVEUiLCJodHUiOiIvYXBpL2RldmljZXMvY2FyZGlvLWExYjJjM2Q0ZTVmNiJ9.XDluue20BrBIDZT1EPSERhzOZNhQkwyVjwCf7CbOZJUxZAumPtpWlJdNWbNttzhxBSBipO0gxqaMQmlduJfXzg";

    #[test]
    fn the_neutral_mac_gives_the_neutral_name() {
        assert_eq!(
            Name::from_mac(NEUTRAL_NAME_MAC).as_str(),
            "cardio-a1b2c3d4e5f6"
        );
    }

    #[test]
    fn registration_body_for_the_neutral_inputs_is_recorded() {
        let name = Name::from_mac(NEUTRAL_NAME_MAC);
        let code = parse_code(NEUTRAL_CODE).unwrap();
        let mut body = String::new();
        registration(&key(1), name.as_str(), &code, &mut body).unwrap();
        assert_eq!(body, NEUTRAL_REGISTRATION);
    }

    #[test]
    fn request_token_for_the_neutral_inputs_is_recorded() {
        let name = Name::from_mac(NEUTRAL_NAME_MAC);
        let path = format!("/api/devices/{}", name.as_str());
        let mut token = String::new();
        request_token(&key(1), name.as_str(), 42, "DELETE", &path, &mut token).unwrap();
        assert_eq!(token, NEUTRAL_TOKEN);
    }

    #[test]
    fn the_registration_body_carries_the_name_and_the_code() {
        let name = Name::from_mac(NEUTRAL_NAME_MAC);
        let code = parse_code(NEUTRAL_CODE).unwrap();
        assert_eq!(
            registration_body(&key(1), &name, &code),
            NEUTRAL_REGISTRATION
        );
    }

    #[test]
    fn the_unpair_authorization_is_a_bearer_token_for_delete() {
        let name = Name::from_mac(NEUTRAL_NAME_MAC);
        let path = unpair_path("/api", &name);
        assert_eq!(
            bearer(&key(1), &name, 42, Method::Delete, &path),
            format!("Bearer {NEUTRAL_TOKEN}")
        );
    }

    #[test]
    fn the_registration_goes_to_devices_under_the_base_path() {
        assert_eq!(registration_path("/api"), "/api/devices");
        assert_eq!(registration_path(""), "/devices");
    }

    #[test]
    fn the_unpair_goes_to_the_registered_name_under_the_base_path() {
        let name = Name::from_mac(NEUTRAL_NAME_MAC);
        assert_eq!(
            unpair_path("/api", &name),
            "/api/devices/cardio-a1b2c3d4e5f6"
        );
        assert_eq!(unpair_path("", &name), "/devices/cardio-a1b2c3d4e5f6");
    }

    #[test]
    fn a_trailing_slash_of_the_backend_url_does_not_double_up_in_the_paths() {
        let base = url::parse("https://backend.example.com/api/").unwrap();
        let name = Name::from_mac(NEUTRAL_NAME_MAC);
        assert_eq!(registration_path(base.path), "/api/devices");
        assert_eq!(
            unpair_path(base.path, &name),
            "/api/devices/cardio-a1b2c3d4e5f6"
        );
    }

    #[test]
    fn a_registration_that_cannot_run_fails_without_a_key() {
        let job = Job::Register(parse_code(NEUTRAL_CODE).unwrap(), key(2));
        assert!(matches!(
            job.failed(),
            Outcome::Registered(PairAnswer::Failed(Failure::Failed))
        ));
    }

    #[test]
    fn an_unpair_that_cannot_run_fails() {
        let job = Job::Unpair(Rc::new(key(1)));
        assert!(matches!(
            job.failed(),
            Outcome::Unpaired(UnpairAnswer::Failed)
        ));
    }
}
