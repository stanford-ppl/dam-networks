use std::{hash::Hash, marker::PhantomData};

use dam::{channel::utils::Peekable, context_tools::*};
use fxhash::FxHashSet;

use super::{policy::Policy, routing::Packet};

#[context_macro]
#[derive(Default)]
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
                    _ => panic!(),
                };
                let targets = self.policy.route(&data.destination());
                let is_ready = occupied_outputs.intersection(&targets).count() == 0;
                if !is_ready {
                    continue;
                }

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
        }
    }
}

enum Event {
    Quit,
    Ready(fxhash::FxHashSet<usize>),
}

impl<T: DAMType, LT, PolicyType> SimpleSwitch<T, LT, PolicyType> {
    fn advance_to_next_event(&mut self) -> Event {
        if self.in_map.is_empty() {
            return Event::Quit;
        }
        if self.in_map.len() == 1 {
            for (id, rcv) in &self.in_map {
                return match rcv.peek_next(&self.time) {
                    Ok(_) => Event::Ready(FxHashSet::from_iter(std::iter::once(*id))),
                    Err(_) => Event::Quit,
                };
            }
            unreachable!();
        } else {
            loop {
                // Loop over all of the channels, jumping forward until at least one of them is ready.
                let next_event = self.in_map.values().next_event();
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
