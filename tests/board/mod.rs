// The firmware's log macros are scoped to this module, so `assert_eq!` in the tests stays core's.

// MUST be the first module
#[path = "../../src/fmt.rs"]
mod fmt;

#[path = "../../src/board/flash.rs"]
pub mod flash;
#[path = "../../src/board/storage.rs"]
pub mod storage;
