/// A Policy is a (possibly) time-varying mapping between target locations and their output ports.
pub trait Policy<LocationType> {
    fn route(&mut self, target: &LocationType) -> fxhash::FxHashSet<usize>;
}

impl<LocationType: Eq + std::hash::Hash> Policy<LocationType>
    for fxhash::FxHashMap<LocationType, fxhash::FxHashSet<usize>>
{
    fn route(&mut self, target: &LocationType) -> fxhash::FxHashSet<usize> {
        match self.get(&target) {
            Some(set) => set.clone(),
            None => panic!("Could not find appropriate routing for location!"),
        }
    }
}
