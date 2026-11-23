//! Bounded, activation-owned operation state for domain backends.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

const MAX_OPERATIONS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub instance_id: String,
    pub component_id: String,
    pub activation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Outcome {
    Committed { item_ids: Vec<String> },
    Failed { reason: String },
    Unknown { item_ids: Vec<String> },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    LimitExceeded,
    InvalidOperation,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::LimitExceeded => "limit-exceeded",
            Self::InvalidOperation => "invalid-operation",
        })
    }
}
impl std::error::Error for Error {}

enum State<W, R> {
    Pending(W),
    Terminal(R),
}
struct Slot<W, R> {
    caller: Caller,
    state: State<W, R>,
    released: bool,
}

pub struct Operations<W, R = Outcome> {
    slots: BTreeMap<String, Slot<W, R>>,
}
impl<W, R> Default for Operations<W, R> {
    fn default() -> Self {
        Self {
            slots: BTreeMap::new(),
        }
    }
}
impl<W, R> Operations<W, R> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn admit(&mut self, token: String, caller: Caller, work: W) -> Result<(), Error> {
        if self.slots.contains_key(&token) {
            return Err(Error::InvalidOperation);
        }
        if self.slots.len() >= MAX_OPERATIONS {
            return Err(Error::LimitExceeded);
        }
        self.slots.insert(
            token,
            Slot {
                caller,
                state: State::Pending(work),
                released: false,
            },
        );
        Ok(())
    }
    pub fn status(&self, token: &str, caller: &Caller) -> Result<Option<&R>, Error> {
        let slot = self
            .slots
            .get(token)
            .filter(|slot| !slot.released && slot.caller == *caller)
            .ok_or(Error::InvalidOperation)?;
        Ok(match &slot.state {
            State::Pending(_) => None,
            State::Terminal(outcome) => Some(outcome),
        })
    }
    pub fn release(&mut self, token: &str, caller: &Caller) -> Result<(), Error> {
        let slot = self
            .slots
            .get_mut(token)
            .filter(|slot| !slot.released && slot.caller == *caller)
            .ok_or(Error::InvalidOperation)?;
        if matches!(slot.state, State::Terminal(_)) {
            self.slots.remove(token);
        } else {
            slot.released = true;
        }
        Ok(())
    }
    pub fn pending_ids(&self) -> Vec<String> {
        self.slots
            .iter()
            .filter(|(_, slot)| matches!(slot.state, State::Pending(_)))
            .map(|(token, _)| token.clone())
            .collect()
    }
    pub fn work_mut(&mut self, token: &str) -> Option<&mut W> {
        match &mut self.slots.get_mut(token)?.state {
            State::Pending(work) => Some(work),
            State::Terminal(_) => None,
        }
    }
    pub fn finish(&mut self, token: &str, outcome: R) -> Result<(), Error> {
        let slot = self.slots.get_mut(token).ok_or(Error::InvalidOperation)?;
        if !matches!(slot.state, State::Pending(_)) {
            return Err(Error::InvalidOperation);
        }
        if slot.released {
            self.slots.remove(token);
        } else {
            slot.state = State::Terminal(outcome);
        }
        Ok(())
    }
    pub fn drain_pending(&mut self) -> Vec<W> {
        std::mem::take(&mut self.slots)
            .into_values()
            .filter_map(|slot| match slot.state {
                State::Pending(work) => Some(work),
                State::Terminal(_) => None,
            })
            .collect()
    }
    pub fn len(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn caller() -> Caller {
        Caller {
            instance_id: "instance".into(),
            component_id: "component".into(),
            activation: 1,
        }
    }
    fn committed() -> Outcome {
        Outcome::Committed {
            item_ids: vec!["item".into()],
        }
    }
    #[test]
    fn released_pending_work_keeps_quota_until_finished() {
        let mut ops = Operations::new();
        let owner = caller();
        for i in 0..16 {
            let id = i.to_string();
            ops.admit(id.clone(), owner.clone(), i).unwrap();
            ops.release(&id, &owner).unwrap();
            assert_eq!(ops.status(&id, &owner), Err(Error::InvalidOperation));
        }
        assert_eq!(ops.pending_ids().len(), 16);
        assert_eq!(
            ops.admit("extra".into(), owner.clone(), 16),
            Err(Error::LimitExceeded)
        );
        ops.finish("0", committed()).unwrap();
        assert_eq!(ops.len(), 15);
        ops.admit("extra".into(), owner, 16).unwrap();
    }
    #[test]
    fn terminal_result_is_retriable_and_activation_owned() {
        let mut ops = Operations::new();
        let owner = caller();
        let mut foreign = owner.clone();
        foreign.activation += 1;
        ops.admit("token".into(), owner.clone(), 1).unwrap();
        assert_eq!(
            ops.admit("token".into(), owner.clone(), 2),
            Err(Error::InvalidOperation)
        );
        assert_eq!(ops.status("token", &foreign), Err(Error::InvalidOperation));
        assert_eq!(ops.release("token", &foreign), Err(Error::InvalidOperation));
        assert_eq!(ops.status("token", &owner), Ok(None));
        ops.finish("token", committed()).unwrap();
        assert_eq!(ops.status("token", &owner), Ok(Some(&committed())));
        assert_eq!(ops.status("token", &owner), Ok(Some(&committed())));
        assert_eq!(
            ops.finish("token", committed()),
            Err(Error::InvalidOperation)
        );
        ops.release("token", &owner).unwrap();
        assert_eq!(ops.status("token", &owner), Err(Error::InvalidOperation));
        assert!(ops.is_empty());
    }
    #[test]
    fn drain_returns_all_pending_including_released_and_discards_terminal() {
        let mut ops = Operations::new();
        let owner = caller();
        for (id, work) in [("a", 1), ("b", 2), ("c", 3)] {
            ops.admit(id.into(), owner.clone(), work).unwrap();
        }
        ops.release("a", &owner).unwrap();
        *ops.work_mut("a").unwrap() = 4;
        ops.finish(
            "c",
            Outcome::Failed {
                reason: "failed".into(),
            },
        )
        .unwrap();
        assert_eq!(ops.drain_pending(), vec![4, 2]);
        assert!(ops.is_empty());
    }
}
