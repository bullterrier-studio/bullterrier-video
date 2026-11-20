//! The ports: video's own contracts for every outside library it relies on. A sibling gets one adapter behind one of these;
//! video's logic names the trait, never the library. Decode and footage maps join them with footage.

pub mod encode;
pub mod music;
pub mod picture;
pub mod signal;
pub mod sound_out;

pub use encode::{Audio, Encode, Job};
pub use music::Music;
pub use picture::Picture;
pub use signal::Signal;
pub use sound_out::Sound;
