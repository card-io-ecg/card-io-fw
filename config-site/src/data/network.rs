#[cfg(feature = "embedded")]
use embedded_io::asynch::{Read, Write};
#[cfg(feature = "embedded")]
use norfs::storable::{LoadError, Loadable, Storable};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WifiNetwork {
    pub ssid: heapless::String<32>,
    pub pass: heapless::String<64>,
}

#[cfg(feature = "embedded")]
impl Loadable for WifiNetwork {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let ssid = heapless::String::<32>::load(reader).await?;
        let pass = heapless::String::<64>::load(reader).await?;
        Ok(Self { ssid, pass })
    }
}

#[cfg(feature = "embedded")]
impl Storable for WifiNetwork {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        self.ssid.store(writer).await?;
        self.pass.store(writer).await?;
        Ok(())
    }
}
