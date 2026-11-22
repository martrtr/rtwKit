//! Session/channel admission, with expiry checked on every credential use.
use std::collections::{HashMap, HashSet, VecDeque};

use crate::{layout::RestoreKey, wire::Request};

const BOOTSTRAP_MS: u64 = 120_000;
const IDLE_MS: u64 = 30 * 60_000;
const MAX_SESSIONS: usize = 4;
const MAX_CHANNELS: usize = 32;
const MAX_PENDING: usize = 64;
const MAX_QUEUED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Owner {
    pub key: RestoreKey,
    pub activation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Query,
    Queryable,
    Incoming,
    Subscription,
    Upload,
    Reader,
    Package,
}
#[derive(Default)]
pub struct Resources(pub HashMap<(ResourceKind, u64), Tracked>);

#[derive(Clone, Copy)]
pub struct Tracked {
    pub parent: Option<u64>,
    pub expires: Option<u64>,
}
impl Resources {
    pub fn owns(&self, kind: ResourceKind, handle: u64) -> bool {
        self.0.contains_key(&(kind, handle))
    }
    pub fn insert(&mut self, kind: ResourceKind, handle: u64, parent: Option<u64>) {
        self.0.insert(
            (kind, handle),
            Tracked {
                parent,
                expires: None,
            },
        );
    }
    pub fn package(&mut self, handle: u64, now: u64) {
        self.0.insert(
            (ResourceKind::Package, handle),
            Tracked {
                parent: None,
                expires: Some(now.saturating_add(120_000)),
            },
        );
    }
    pub fn remove(&mut self, kind: ResourceKind, handle: u64) {
        self.0.remove(&(kind, handle));
        if kind == ResourceKind::Queryable {
            self.0.retain(|(kind, _), parent| {
                *kind != ResourceKind::Incoming || parent.parent != Some(handle)
            });
        }
    }
    pub fn incoming(&mut self, handle: u64, parent: u64, now: u64) {
        self.0.insert(
            (ResourceKind::Incoming, handle),
            Tracked {
                parent: Some(parent),
                expires: Some(now.saturating_add(500)),
            },
        );
    }
    pub fn expire_incoming(&mut self, now: u64) {
        self.0
            .retain(|_, tracked| !tracked.expires.is_some_and(|until| now >= until));
    }
    pub fn append(&mut self, other: Self) {
        self.0.extend(other.0);
    }
}
struct Session {
    owner: Owner,
    connections: HashSet<u64>,
    idle_since: Option<u64>,
}
pub struct Channel {
    pub owner: Owner,
    pub shell: bool,
    pub connection: u64,
    token: String,
    pending: HashMap<String, usize>,
    queue: VecDeque<Request>,
    pub resources: Resources,
}
#[derive(Default)]
pub struct Sessions {
    bootstrap: Option<(String, u64)>,
    sessions: HashMap<String, Session>,
    pub channels: HashMap<String, Channel>,
    pending: usize,
    queued_bytes: usize,
}
pub struct Ready {
    pub token: String,
    pub channel: String,
}
impl Sessions {
    /// Tokens/channel IDs are supplied by the Host entropy boundary, never a client.
    pub fn bootstrap(&mut self, token: String, now: u64) {
        self.bootstrap = Some((token, now.saturating_add(BOOTSTRAP_MS)));
    }
    pub fn needs_bootstrap(&self) -> bool {
        self.bootstrap.is_none() && self.sessions.len() < MAX_SESSIONS
    }
    pub fn authenticate(
        &mut self,
        token: &str,
        connection: u64,
        now: u64,
        owner: &Owner,
        new_token: String,
        channel: String,
    ) -> Result<Ready, &'static str> {
        self.expire(now);
        if self.channels.values().any(|c| c.connection == connection)
            || self.channels.contains_key(&channel)
        {
            return Err("protocol-error");
        }
        let existing = self.sessions.get(token);
        let token = if let Some(existing) = existing {
            if existing.owner != *owner
                || existing
                    .idle_since
                    .is_some_and(|at| now.saturating_sub(at) >= IDLE_MS)
            {
                return Err("channel-closed");
            }
            token.to_string()
        } else {
            let Some((expected, deadline)) = &self.bootstrap else {
                return Err("channel-closed");
            };
            if now >= *deadline
                || !constant_time_equal(expected.as_bytes(), token.as_bytes())
                || self.sessions.len() >= MAX_SESSIONS
                || self.sessions.contains_key(&new_token)
            {
                return Err("channel-closed");
            }
            new_token
        };
        if !self.sessions.contains_key(&token) {
            self.bootstrap = None;
            self.sessions.insert(
                token.clone(),
                Session {
                    owner: owner.clone(),
                    connections: HashSet::new(),
                    idle_since: None,
                },
            );
        }
        let session = self.sessions.get_mut(&token).unwrap();
        session.connections.insert(connection);
        session.idle_since = None;
        self.channels.insert(
            channel.clone(),
            Channel {
                owner: owner.clone(),
                shell: true,
                connection,
                token: token.clone(),
                pending: HashMap::new(),
                queue: VecDeque::new(),
                resources: Resources::default(),
            },
        );
        Ok(Ready { token, channel })
    }
    pub fn check(
        &self,
        channel: &str,
        connection: u64,
        now: u64,
    ) -> Result<&Channel, &'static str> {
        let channel = self.channels.get(channel).ok_or("channel-closed")?;
        let session = self.sessions.get(&channel.token).ok_or("channel-closed")?;
        if channel.connection != connection
            || !session.connections.contains(&connection)
            || session
                .idle_since
                .is_some_and(|at| now.saturating_sub(at) >= IDLE_MS)
        {
            return Err("channel-closed");
        }
        Ok(channel)
    }
    pub fn mount(
        &mut self,
        parent: &str,
        connection: u64,
        now: u64,
        id: String,
        owner: Owner,
    ) -> Result<(), &'static str> {
        let parent = self.check(parent, connection, now)?;
        if !parent.shell {
            return Err("invalid-request");
        }
        let token = parent.token.clone();
        if self.channels.contains_key(&id)
            || self.channels.values().filter(|c| !c.shell).count() >= MAX_CHANNELS
        {
            return Err("limit-exceeded");
        }
        self.channels.insert(
            id,
            Channel {
                owner,
                shell: false,
                connection,
                token,
                pending: HashMap::new(),
                queue: VecDeque::new(),
                resources: Resources::default(),
            },
        );
        Ok(())
    }
    pub fn admit(
        &mut self,
        request: Request,
        connection: u64,
        now: u64,
        bytes: usize,
    ) -> Result<(), &'static str> {
        let channel = self.check(&request.channel, connection, now)?;
        if request.call.shell_only() && !channel.shell {
            return Err("invalid-request");
        }
        if channel.pending.contains_key(&request.id) {
            return Err("protocol-error");
        }
        if channel.pending.len() >= 8
            || self.pending >= MAX_PENDING
            || self.queued_bytes.saturating_add(bytes) > MAX_QUEUED_BYTES
        {
            return Err("limit-exceeded");
        }
        let channel = self.channels.get_mut(&request.channel).unwrap();
        channel.pending.insert(request.id.clone(), bytes);
        channel.queue.push_back(request);
        self.pending += 1;
        self.queued_bytes += bytes;
        Ok(())
    }
    pub fn take(&mut self, channel: &str, owner: &Owner, now: u64) -> Option<Request> {
        let connection = self.channels.get(channel)?.connection;
        if self.check(channel, connection, now).is_err() {
            return None;
        }
        let channel = self.channels.get_mut(channel)?;
        if channel.owner != *owner {
            return None;
        }
        channel.queue.pop_front()
    }
    pub fn complete(&mut self, channel: &str, id: &str) {
        if let Some(bytes) = self
            .channels
            .get_mut(channel)
            .and_then(|c| c.pending.remove(id))
        {
            self.pending -= 1;
            self.queued_bytes -= bytes;
        }
    }
    pub fn may_unmount(&self, parent: &str, target: &str, connection: u64, now: u64) -> bool {
        match (
            self.check(parent, connection, now),
            self.check(target, connection, now),
        ) {
            (Ok(parent), Ok(target)) => {
                parent.shell && !target.shell && parent.token == target.token
            }
            _ => false,
        }
    }
    pub fn close_channel(&mut self, id: &str) -> Option<(Owner, Resources)> {
        let channel = self.channels.remove(id)?;
        for bytes in channel.pending.into_values() {
            self.pending -= 1;
            self.queued_bytes -= bytes;
        }
        Some((channel.owner, channel.resources))
    }
    pub fn disconnect(&mut self, connection: u64, now: u64) -> Vec<(Owner, Resources)> {
        for session in self.sessions.values_mut() {
            if session.connections.remove(&connection) && session.connections.is_empty() {
                session.idle_since = Some(now);
            }
        }
        let channels: Vec<_> = self
            .channels
            .iter()
            .filter(|(_, c)| c.connection == connection)
            .map(|(id, _)| id.clone())
            .collect();
        channels
            .iter()
            .filter_map(|id| self.close_channel(id))
            .collect()
    }
    pub fn session_connections(
        &self,
        parent: &str,
        connection: u64,
        now: u64,
    ) -> Result<Vec<u64>, &'static str> {
        let channel = self.check(parent, connection, now)?;
        if !channel.shell {
            return Err("invalid-request");
        }
        Ok(self.sessions[&channel.token]
            .connections
            .iter()
            .copied()
            .collect())
    }
    pub fn disconnect_without_clock(&mut self, connection: u64) -> Vec<(Owner, Resources)> {
        self.sessions.retain(|_, session| {
            !session.connections.remove(&connection) || !session.connections.is_empty()
        });
        let ids: Vec<_> = self
            .channels
            .iter()
            .filter(|(_, channel)| channel.connection == connection)
            .map(|(id, _)| id.clone())
            .collect();
        ids.iter().filter_map(|id| self.close_channel(id)).collect()
    }
    pub fn logout(
        &mut self,
        parent: &str,
        connection: u64,
        now: u64,
    ) -> Result<Vec<(Owner, Resources)>, &'static str> {
        let channel = self.check(parent, connection, now)?;
        if !channel.shell {
            return Err("invalid-request");
        }
        let token = channel.token.clone();
        self.sessions.remove(&token);
        let channels: Vec<_> = self
            .channels
            .iter()
            .filter(|(_, c)| c.token == token)
            .map(|(id, _)| id.clone())
            .collect();
        Ok(channels
            .iter()
            .filter_map(|id| self.close_channel(id))
            .collect())
    }
    pub fn expire(&mut self, now: u64) {
        if self
            .bootstrap
            .as_ref()
            .is_some_and(|(_, deadline)| now >= *deadline)
        {
            self.bootstrap = None;
        }
        self.sessions.retain(|_, session| {
            !session
                .idle_since
                .is_some_and(|at| now.saturating_sub(at) >= IDLE_MS)
        });
    }
}
fn constant_time_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b)
        .fold(0u8, |different, (a, b)| different | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Call;

    fn request(channel: &str, id: &str) -> Request {
        Request {
            id: id.into(),
            channel: channel.into(),
            call: Call::PollQuery(crate::wire::Handle(1)),
        }
    }

    fn shell(state: &mut Sessions, bootstrap: &str, token: &str, connection: u64, channel: &str) {
        state.bootstrap(bootstrap.into(), 0);
        state
            .authenticate(
                bootstrap,
                connection,
                0,
                &owner(),
                token.into(),
                channel.into(),
            )
            .unwrap();
    }
    fn owner() -> Owner {
        Owner {
            key: RestoreKey {
                world_id: None,
                instance_id: "instance".into(),
                component_id: "shell".into(),
            },
            activation: 1,
        }
    }
    #[test]
    fn auth_expiry_is_checked_before_use_without_any_cleanup_tick() {
        let mut state = Sessions::default();
        state.bootstrap("launch".into(), 0);
        assert!(
            state
                .authenticate(
                    "launch",
                    1,
                    BOOTSTRAP_MS,
                    &owner(),
                    "session".into(),
                    "shell".into()
                )
                .is_err()
        );
        state.bootstrap("fresh".into(), BOOTSTRAP_MS);
        state
            .authenticate(
                "fresh",
                1,
                BOOTSTRAP_MS,
                &owner(),
                "session".into(),
                "shell".into(),
            )
            .unwrap();
        assert!(
            state
                .authenticate(
                    "fresh",
                    2,
                    BOOTSTRAP_MS,
                    &owner(),
                    "second".into(),
                    "other".into()
                )
                .is_err()
        );
        state.disconnect(1, BOOTSTRAP_MS);
        assert!(
            state
                .authenticate(
                    "session",
                    2,
                    BOOTSTRAP_MS + IDLE_MS,
                    &owner(),
                    "unused".into(),
                    "other".into()
                )
                .is_err()
        );
    }
    #[test]
    fn reconnect_reuses_token_but_not_channels_and_exact_connection_owns_resources() {
        let mut state = Sessions::default();
        state.bootstrap("launch".into(), 0);
        let ready = state
            .authenticate("launch", 1, 0, &owner(), "session".into(), "shell".into())
            .unwrap();
        state
            .mount(&ready.channel, 1, 0, "panel".into(), owner())
            .unwrap();
        assert!(state.check("panel", 2, 0).is_err());
        assert!(
            state
                .mount("panel", 1, 0, "forged".into(), owner())
                .is_err()
        );
        let released = state.disconnect(1, 5);
        assert_eq!(released.len(), 2);
        let fresh = state
            .authenticate(
                "session",
                2,
                6,
                &owner(),
                "unused".into(),
                "new-shell".into(),
            )
            .unwrap();
        assert_eq!(fresh.token, ready.token);
        assert_ne!(fresh.channel, ready.channel);
        assert!(state.check("panel", 2, 6).is_err());
        state.logout(&fresh.channel, 2, 6).unwrap();
        assert!(
            state
                .authenticate("session", 3, 7, &owner(), "unused".into(), "new".into())
                .is_err()
        );
    }

    #[test]
    fn logout_releases_all_session_channels_without_affecting_other_session() {
        let mut state = Sessions::default();
        shell(&mut state, "launch-a", "a", 1, "a1");
        state
            .authenticate("a", 2, 0, &owner(), "unused".into(), "a2".into())
            .unwrap();
        state.mount("a1", 1, 0, "panel-a".into(), owner()).unwrap();
        shell(&mut state, "launch-b", "b", 3, "b1");
        state.mount("b1", 3, 0, "panel-b".into(), owner()).unwrap();
        for (channel, handle) in [("a1", 1), ("a2", 2), ("panel-a", 3), ("panel-b", 4)] {
            state.channels.get_mut(channel).unwrap().resources.insert(
                ResourceKind::Query,
                handle,
                None,
            );
        }
        let released = state.logout("a1", 1, 0).unwrap();
        assert_eq!(released.len(), 3);
        for handle in 1..=3 {
            assert!(
                released
                    .iter()
                    .any(|(_, resources)| resources.owns(ResourceKind::Query, handle))
            );
        }
        for (channel, connection) in [("a1", 1), ("a2", 2), ("panel-a", 1)] {
            assert!(state.check(channel, connection, 0).is_err());
        }
        assert!(
            state
                .authenticate("a", 4, 0, &owner(), "unused".into(), "a3".into())
                .is_err()
        );
        assert!(state.check("b1", 3, 0).is_ok());
        assert!(
            state
                .check("panel-b", 3, 0)
                .unwrap()
                .resources
                .owns(ResourceKind::Query, 4)
        );
        assert!(
            state
                .authenticate("b", 4, 0, &owner(), "unused".into(), "b2".into())
                .is_ok()
        );
    }

    #[test]
    fn admission_limits_and_disconnect_restore_quota() {
        let mut state = Sessions::default();
        shell(&mut state, "launch", "session", 1, "shell");
        for i in 0..8 {
            state
                .admit(request("shell", &format!("r{i}")), 1, 0, 1)
                .unwrap();
        }
        assert_eq!(
            state.admit(request("shell", "r0"), 1, 0, 1),
            Err("protocol-error")
        );
        assert_eq!(
            state.admit(request("shell", "overflow"), 1, 0, 1),
            Err("limit-exceeded")
        );
        state.complete("shell", "r0");
        state
            .admit(request("shell", "replacement"), 1, 0, 1)
            .unwrap();
        for i in 0..7 {
            let channel = format!("panel{i}");
            state
                .mount("shell", 1, 0, channel.clone(), owner())
                .unwrap();
            for j in 0..8 {
                state
                    .admit(request(&channel, &format!("r{j}")), 1, 0, 1)
                    .unwrap();
            }
        }
        state.mount("shell", 1, 0, "extra".into(), owner()).unwrap();
        assert_eq!(
            state.admit(request("extra", "r"), 1, 0, 1),
            Err("limit-exceeded")
        );
        state.complete("panel0", "r0");
        state.admit(request("extra", "r"), 1, 0, 1).unwrap();
        state.disconnect(1, 0);
        state
            .authenticate("session", 2, 1, &owner(), "unused".into(), "fresh".into())
            .unwrap();
        state
            .admit(request("fresh", "large"), 2, 1, MAX_QUEUED_BYTES)
            .unwrap();
        assert_eq!(
            state.admit(request("fresh", "extra"), 2, 1, 1),
            Err("limit-exceeded")
        );
        state.complete("fresh", "large");
        state
            .admit(request("fresh", "extra"), 2, 1, MAX_QUEUED_BYTES)
            .unwrap();
        state.disconnect(2, 2);
        state
            .authenticate("session", 3, 3, &owner(), "unused".into(), "last".into())
            .unwrap();
        state
            .admit(request("last", "large"), 3, 3, MAX_QUEUED_BYTES)
            .unwrap();
    }

    #[test]
    fn stale_activation_cannot_take_new_owner_request() {
        let mut state = Sessions::default();
        shell(&mut state, "launch", "session", 1, "shell");
        let old_owner = owner();
        let mut new_owner = old_owner.clone();
        new_owner.activation = 2;
        state
            .mount("shell", 1, 0, "panel".into(), new_owner.clone())
            .unwrap();
        state.admit(request("panel", "r"), 1, 0, 1).unwrap();
        assert!(state.take("panel", &old_owner, 0).is_none());
        assert_eq!(state.take("panel", &new_owner, 0).unwrap().id, "r");
    }
}
