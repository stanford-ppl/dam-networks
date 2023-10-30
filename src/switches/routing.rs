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
