use dam::context_tools::*;

pub trait Packet<LocationType> {
    fn destination(&self) -> LocationType;
}

pub struct Port<ElementType: Clone> {
    pub id: usize,
    pub input: Option<Receiver<ElementType>>,
    pub output: Option<Sender<ElementType>>,
}

pub trait Switch<ElementType: Clone> {
    fn add_port(&mut self, port: Port<ElementType>);
}

#[derive(Copy, Clone, Debug, Default)]
pub struct SimplePacket<LocationType, PayloadType> {
    pub location: LocationType,
    pub payload: PayloadType,
}

impl<LT: Clone, PT> Packet<LT> for SimplePacket<LT, PT> {
    fn destination(&self) -> LT {
        self.location.clone()
    }
}

impl<LT: DAMType, PT: DAMType> DAMType for SimplePacket<LT, PT> {
    fn dam_size(&self) -> usize {
        self.location.dam_size() + self.payload.dam_size()
    }
}
