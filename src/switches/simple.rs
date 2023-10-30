use std::{hash::Hash, marker::PhantomData};

use dam::{channel::utils::Peekable, context_tools::*};
use fxhash::FxHashSet;

use super::{
    policy::Policy,
    routing::{Packet, Port},
};

#[context_macro]
struct SimpleSwitch<T, LT, PolicyType>
where
    T: DAMType,
{
    in_map: fxhash::FxHashMap<usize, Receiver<T>>,
    out_map: fxhash::FxHashMap<usize, Sender<T>>,

    policy: PolicyType,
    latency: u64,

    _marker: PhantomData<LT>,
}

impl<T: DAMType, LT, PolicyType> Context for SimpleSwitch<T, LT, PolicyType>
where
    T: Packet<LT>,
    LT: Sync + Send + Eq + Hash,
    PolicyType: Policy<LT> + Sync + Send,
{
    fn run(&mut self) {
        loop {
            let ready = match self.advance_to_next_event() {
                Event::Quit => return,
                Event::Ready(set) => set,
            };

            let mut occupied_outputs = fxhash::FxHashSet::default();
            for input_port in ready {
                let data = match self.in_map.get(&input_port).unwrap().peek() {
                    dam::channel::PeekResult::Something(ChannelElement { time: _, data }) => data,
                    _ => panic!("Port {:?} was supposed to be ready", input_port),
                };
                let targets = self.policy.route(&data.destination());
                let is_ready = occupied_outputs.intersection(&targets).count() == 0;
                if !is_ready {
                    continue;
                }

                // Pop it off since it's ready.
                let _ = self.in_map.get(&input_port).unwrap().dequeue(&self.time);

                targets.iter().for_each(|x| {
                    let _ = self
                        .out_map
                        .get(x)
                        .unwrap()
                        .wait_until_available(&self.time);
                });

                targets.iter().for_each(|x| {
                    let _ = self.out_map.get(x).unwrap().enqueue(
                        &self.time,
                        ChannelElement {
                            time: self.time.tick() + self.latency,
                            data: data.clone(),
                        },
                    );
                });

                // Add the targets to the occupied set.
                occupied_outputs.extend(targets);
            }
            self.time.incr_cycles(1);
        }
    }
}

enum Event {
    Quit,
    Ready(fxhash::FxHashSet<usize>),
}

impl<T: DAMType, LT, PolicyType> SimpleSwitch<T, LT, PolicyType>
where
    Self: Context,
{
    pub fn new(policy: PolicyType, latency: u64) -> Self {
        Self {
            in_map: Default::default(),
            out_map: Default::default(),
            policy,
            latency,
            _marker: Default::default(),
            context_info: Default::default(),
        }
    }

    pub fn add_port(&mut self, port: Port<T>) {
        let id = port.id;
        if let Some(rcv) = port.input {
            rcv.attach_receiver(self);
            assert!(
                self.in_map.insert(id, rcv).is_none(),
                "Input port was already occupied!"
            );
        }
        if let Some(snd) = port.output {
            snd.attach_sender(self);
            assert!(
                self.out_map.insert(id, snd).is_none(),
                "Output port was already occupied!"
            );
        }
    }

    fn advance_to_next_event(&mut self) -> Event {
        if self.in_map.is_empty() {
            return Event::Quit;
        }
        if self.in_map.len() == 1 {
            if let Some((id, rcv)) = self.in_map.iter().next() {
                return match rcv.peek_next(&self.time) {
                    Ok(_) => Event::Ready(FxHashSet::from_iter(std::iter::once(*id))),
                    Err(_) => Event::Quit,
                };
            } else {
                unreachable!("We just checked that the in map had one element");
            }
        } else {
            loop {
                // Loop over all of the channels, jumping forward until at least one of them is ready.
                let next_event = self.in_map.values().map(|chan| chan.next_event()).min().unwrap();

                match next_event {
                    dam::channel::utils::EventTime::Ready(t) => {
                        // Hop ourselves forward to the ready time.
                        self.time.advance(t);
                        // Now filter the channels to see which ones were ready
                        return Event::Ready(
                            self.in_map
                                .values()
                                .enumerate()
                                .filter(|(_, chan)| match chan.peek() {
                                    // Get all of the channels which had something on them and are ready
                                    dam::channel::PeekResult::Something(x) if x.time <= t => true,
                                    _ => false,
                                })
                                // Get the index of those channels
                                .map(|(ind, _)| ind)
                                .collect(),
                        );
                    }
                    // If there's nothing ready, hop forward one tick after.
                    // We could be a bit more intelligent w.r.t. the channels' latency, but that feels error prone
                    // and I'm not sure if we need to do that right now. (10/29/23)
                    dam::channel::utils::EventTime::Nothing(t) => self.time.advance(t + 1),
                    dam::channel::utils::EventTime::Closed => return Event::Quit,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use dam::{simulation::{ProgramBuilder, DotConvertible}, utility_contexts::*, context_tools::ChannelElement};
    use fxhash::FxHashSet;

    use crate::switches::{routing::{SimplePacket, Port}, simple::SimpleSwitch};

    #[test]
    fn simple_switch_test() {
        const NUM_PACKETS: u16 = 32;

        let mut ctx = ProgramBuilder::default();

        let (g2switch_snd, g2switch_rcv) = ctx.unbounded();
        ctx.add_child(GeneratorContext::new(
            || {
                (0..NUM_PACKETS).map(|i| 
                // Hardcode location to port 1
                SimplePacket {
                    location: 1u8,
                    payload: i,
                })
            },
            g2switch_snd,
        ));

        // Maps 1 -> {1}, 2 -> {2}
        let policy = fxhash::FxHashMap::from_iter([(1u8, FxHashSet::from_iter([1usize])), (2, FxHashSet::from_iter([2usize]))]);
        let mut switch = SimpleSwitch::new(policy, 1);
        switch.add_port(Port { id: 0, input: Some(g2switch_rcv), output: None });

        let (switch2comp_snd, switch2comp_rcv) = ctx.unbounded();
        let (comp2switch_snd, comp2switch_rcv) = ctx.unbounded();

        switch.add_port(Port {id: 1, input: Some(comp2switch_rcv), output: Some(switch2comp_snd)});

        let mut comp = FunctionContext::new();
        comp2switch_snd.attach_sender(&comp);
        switch2comp_rcv.attach_receiver(&comp);
        comp.set_run(move |time| {
            for i in 0..NUM_PACKETS {
                let ChannelElement { time: _, data: SimplePacket { location: _, payload } } = switch2comp_rcv.dequeue(time).unwrap();
                comp2switch_snd.enqueue(time, ChannelElement { time: time.tick() + 1, data: SimplePacket { location: 2, payload: payload + i + 100 } }).unwrap();
                time.incr_cycles(1);
            }
        });
        ctx.add_child(comp);

        let (switch2check_snd, switch2check_rcv) = ctx.unbounded();
        ctx.add_child(PrinterContext::new(switch2check_rcv));

        switch.add_port(Port { id: 2, input: None, output: Some(switch2check_snd) });
        ctx.add_child(switch);

        let initialized = ctx.initialize(Default::default()).unwrap();
        println!("{}", initialized.to_dot_string());
        initialized.run(Default::default());

    }
}
