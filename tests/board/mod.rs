// The firmware's log macros are scoped to this module, so `assert_eq!` in the tests stays core's.

// MUST be the first module
#[path = "../../src/fmt.rs"]
mod fmt;

#[path = "../../src/board/flash.rs"]
pub mod flash;
// The tests save no record and never panic through the firmware's handler.
#[allow(dead_code)]
#[path = "../../src/board/logging.rs"]
pub mod logging;
#[path = "../../src/board/storage.rs"]
pub mod storage;
