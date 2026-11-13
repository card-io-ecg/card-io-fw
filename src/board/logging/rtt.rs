use rtt_target::{rtt_init, ChannelMode, UpChannel};

/// RTT, which probe-rs reads.
pub struct Transport {
    channel: Option<UpChannel>,
}

impl Transport {
    pub const fn new() -> Self {
        Self { channel: None }
    }

    pub fn init(&mut self) {
        let channels = rtt_init! {
            up: {
                0: {
                    size: 1024,
                    mode: ChannelMode::NoBlockSkip,
                    name: "defmt"
                }
            }
        };

        self.channel = Some(channels.up.0);
    }

    pub fn start_frame(&mut self) {}

    pub fn write(&mut self, bytes: &[u8]) {
        if let Some(channel) = self.channel.as_mut() {
            channel.write(bytes);
        }
    }

    pub fn end_frame(&mut self) {}
}
