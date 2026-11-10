//! In-memory topic tree with per-topic message history.

use std::collections::{BTreeMap, HashSet, VecDeque};

use chrono::{DateTime, Local};

use crate::model::MessageRecord;
use crate::util::payload_preview;

const MAX_VISIBLE_ROWS: usize = 20_000;
const MAX_TOPIC_HISTORY_BYTES: u64 = 512 * 1024;

/// Key for the synthetic node that groups topics starting with `/`.
///
/// Such topics have an empty first segment. That node has no MQTT path of its
/// own, and an empty path already denotes the invisible root, so the UI
/// addresses it through this key. MQTT topic names must not contain NUL, which
/// keeps the key collision-free.
pub const LEADING_SLASH_GROUP: &str = "\0";

/// Returns a path as it should be shown to the user.
pub fn display_path(path: &str) -> &str {
    if path.is_empty() || path == LEADING_SLASH_GROUP {
        "/"
    } else {
        path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    NewTopic,
    Updated,
    Removed,
    Ignored,
}

/// How the "since" filter treats a topic's latest message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SinceMode {
    /// The time filter is disabled.
    #[default]
    AnyTime,
    /// Show topics that received a message at or after the cutoff.
    ReceivedSince,
    /// Show topics whose last message is older than the cutoff.
    NoMessagesSince,
}

/// A resolved "since" filter: a mode plus the cutoff timestamp.
#[derive(Debug, Clone, Copy)]
pub struct SinceFilter {
    pub mode: SinceMode,
    pub cutoff: DateTime<Local>,
}

impl SinceFilter {
    fn matches(&self, data: &TopicData) -> bool {
        // Retained messages replayed at subscribe time are broker state, not
        // live activity, so they do not count for the time filter.
        match self.mode {
            SinceMode::AnyTime => true,
            SinceMode::ReceivedSince => data
                .last_live_seen
                .is_some_and(|last_seen| last_seen >= self.cutoff),
            SinceMode::NoMessagesSince => data
                .last_live_seen
                .is_none_or(|last_seen| last_seen < self.cutoff),
        }
    }
}

#[derive(Debug, Default)]
pub struct TopicTree {
    pub root: TopicNode,
    pub node_count: usize,
    pub value_topic_count: usize,
    pub received_messages: u64,
    pub payload_bytes: u64,
    pub last_message_at: Option<DateTime<Local>>,
}

#[derive(Debug)]
pub struct TopicNode {
    pub segment: String,
    pub path: String,
    pub children: BTreeMap<String, TopicNode>,
    pub data: Option<TopicData>,
    pub expanded: bool,
}

impl Default for TopicNode {
    fn default() -> Self {
        Self::new(String::new(), String::new())
    }
}

impl TopicNode {
    fn new(segment: String, path: String) -> Self {
        Self {
            segment,
            path,
            children: BTreeMap::new(),
            data: None,
            // Every branch starts collapsed; the user expands it manually.
            expanded: false,
        }
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

#[derive(Debug)]
pub struct TopicData {
    pub history: VecDeque<MessageRecord>,
    pub total: u64,
    pub first_seen: DateTime<Local>,
    pub last_seen: DateTime<Local>,
    /// Receipt time of the most recent live (non-backlog) message.
    pub last_live_seen: Option<DateTime<Local>>,
    pub preview: String,
    pub bytes: u64,
}

impl TopicData {
    fn empty(timestamp: DateTime<Local>) -> Self {
        Self {
            history: VecDeque::new(),
            total: 0,
            first_seen: timestamp,
            last_seen: timestamp,
            last_live_seen: None,
            preview: String::new(),
            bytes: 0,
        }
    }

    pub fn latest(&self) -> Option<&MessageRecord> {
        self.history.back()
    }

    pub fn previous(&self) -> Option<&MessageRecord> {
        let len = self.history.len();
        if len >= 2 {
            self.history.get(len - 2)
        } else {
            None
        }
    }

    fn push(&mut self, message: MessageRecord, max_history: usize) {
        let max_history = max_history.clamp(10, 5_000);
        self.total = self.total.saturating_add(1);
        self.last_seen = message.received_at;
        if !message.backlog {
            self.last_live_seen = Some(message.received_at);
        }
        self.preview = payload_preview(&message.payload, 96);
        self.bytes = self.bytes.saturating_add(message.size() as u64);
        self.history.push_back(message);

        while self.history.len() > max_history
            || (self.bytes > MAX_TOPIC_HISTORY_BYTES && self.history.len() > 1)
        {
            let Some(old) = self.history.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(old.size() as u64);
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.bytes = 0;
        self.preview.clear();
    }
}

#[derive(Debug)]
pub struct TopicRow<'a> {
    pub path: &'a str,
    pub segment: &'a str,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub total: u64,
    pub stored: usize,
    pub preview: &'a str,
    pub retained: bool,
    pub has_data: bool,
}

impl TopicTree {
    pub fn insert(&mut self, message: MessageRecord, max_history: usize) -> InsertOutcome {
        if message.topic.is_empty() || message.topic.contains('\0') {
            return InsertOutcome::Ignored;
        }

        // An empty retained payload clears the retained value on the broker.
        if message.retain && message.payload.is_empty() {
            return if self.clear_topic_data(&message.topic) {
                InsertOutcome::Removed
            } else {
                InsertOutcome::Ignored
            };
        }

        let payload_size = message.size() as u64;
        let received_at = message.received_at;
        let mut new_nodes = 0usize;
        let mut new_value_topic = false;

        {
            let mut node = &mut self.root;
            let mut path = String::with_capacity(message.topic.len());

            for (depth, segment) in message.topic.split('/').enumerate() {
                if depth > 0 {
                    path.push('/');
                }
                path.push_str(segment);

                if !node.children.contains_key(segment) {
                    let child = TopicNode::new(segment.to_owned(), path.clone());
                    node.children.insert(segment.to_owned(), child);
                    new_nodes += 1;
                }
                node = node
                    .children
                    .get_mut(segment)
                    .expect("topic node was just inserted");
            }

            if node.data.is_none() {
                node.data = Some(TopicData::empty(received_at));
                new_value_topic = true;
            }
            let data = node.data.as_mut().expect("topic data was just inserted");
            data.push(message, max_history);
        }

        self.node_count += new_nodes;
        if new_value_topic {
            self.value_topic_count += 1;
        }
        self.received_messages = self.received_messages.saturating_add(1);
        self.payload_bytes = self.payload_bytes.saturating_add(payload_size);
        self.last_message_at = Some(received_at);

        if new_value_topic {
            InsertOutcome::NewTopic
        } else {
            InsertOutcome::Updated
        }
    }

    pub fn node(&self, path: &str) -> Option<&TopicNode> {
        if path == LEADING_SLASH_GROUP {
            return self.root.children.get("");
        }
        if path.is_empty() {
            return Some(&self.root);
        }
        let mut node = &self.root;
        for segment in path.split('/') {
            node = node.children.get(segment)?;
        }
        Some(node)
    }

    fn node_mut(&mut self, path: &str) -> Option<&mut TopicNode> {
        if path == LEADING_SLASH_GROUP {
            return self.root.children.get_mut("");
        }
        if path.is_empty() {
            return Some(&mut self.root);
        }
        let mut node = &mut self.root;
        for segment in path.split('/') {
            node = node.children.get_mut(segment)?;
        }
        Some(node)
    }

    pub fn data(&self, path: &str) -> Option<&TopicData> {
        self.node(path).and_then(|node| node.data.as_ref())
    }

    /// Collects the exact topic paths at or below `prefix` whose latest
    /// message is retained. This is the set of topics that a branch-wide
    /// retained cleanup has to clear on the broker.
    pub fn retained_topics_under(&self, prefix: &str) -> Vec<String> {
        fn walk(node: &TopicNode, topics: &mut Vec<String>) {
            if let Some(data) = &node.data
                && data.latest().is_some_and(|message| message.retain)
            {
                topics.push(node.path.clone());
            }
            for child in node.children.values() {
                walk(child, topics);
            }
        }

        let mut topics = Vec::new();
        if let Some(node) = self.node(prefix) {
            walk(node, &mut topics);
        }
        topics.sort();
        topics
    }

    /// Restores the message counter for a topic loaded from a snapshot.
    pub fn set_total(&mut self, path: &str, total: u64) {
        if let Some(data) = self.node_mut(path).and_then(|node| node.data.as_mut()) {
            data.total = total.max(1);
        }
    }

    pub fn toggle_expanded(&mut self, path: &str) {
        if let Some(node) = self.node_mut(path) {
            node.expanded = !node.expanded;
        }
    }

    pub fn set_all_expanded(&mut self, expanded: bool) {
        fn walk(node: &mut TopicNode, expanded: bool) {
            node.expanded = expanded;
            for child in node.children.values_mut() {
                walk(child, expanded);
            }
        }
        walk(&mut self.root, expanded);
    }

    pub fn remove_topic(&mut self, path: &str) -> bool {
        // Deleting the leading-slash group deletes every topic below it.
        if path == LEADING_SLASH_GROUP {
            let removed = self.root.children.remove("").is_some();
            if removed {
                self.recount();
            }
            return removed;
        }
        if path.is_empty() {
            return false;
        }

        let segments: Vec<&str> = path.split('/').collect();
        let Some((last, parents)) = segments.split_last() else {
            return false;
        };

        let mut parent = &mut self.root;
        for segment in parents {
            match parent.children.get_mut(*segment) {
                Some(node) => parent = node,
                None => return false,
            }
        }

        let removed = parent.children.remove(*last).is_some();
        if removed {
            prune_empty(&mut self.root);
            self.recount();
        }
        removed
    }

    pub fn clear_topic_data(&mut self, path: &str) -> bool {
        let Some(node) = self.node_mut(path) else {
            return false;
        };
        if node.data.take().is_none() {
            return false;
        }
        prune_empty(&mut self.root);
        self.recount();
        true
    }

    pub fn clear_history(&mut self, path: &str) {
        if let Some(data) = self.node_mut(path).and_then(|node| node.data.as_mut()) {
            data.clear_history();
        }
    }

    pub fn clear(&mut self) {
        self.root = TopicNode::default();
        self.root.expanded = true;
        self.node_count = 0;
        self.value_topic_count = 0;
        self.received_messages = 0;
        self.payload_bytes = 0;
        self.last_message_at = None;
    }

    /// Collects the rows currently visible in the tree.
    ///
    /// The text filter and the since filter are combined: with any filter
    /// active, only matching topics and their ancestors are returned.
    pub fn visible_rows(
        &self,
        filter: &str,
        search_payload: bool,
        since: Option<SinceFilter>,
    ) -> Vec<TopicRow<'_>> {
        let needle = filter.trim().to_lowercase();
        let mut rows = Vec::new();

        if needle.is_empty() && since.is_none() {
            for child in self.root.children.values() {
                push_visible(child, 0, &mut rows);
            }
        } else {
            for child in self.root.children.values() {
                collect_filtered(child, 0, &needle, search_payload, since, &mut rows);
            }
        }

        if rows.len() > MAX_VISIBLE_ROWS {
            rows.truncate(MAX_VISIBLE_ROWS);
        }
        rows
    }

    fn recount(&mut self) {
        fn walk(node: &TopicNode, nodes: &mut usize, value_topics: &mut usize) {
            for child in node.children.values() {
                *nodes += 1;
                if child.data.is_some() {
                    *value_topics += 1;
                }
                walk(child, nodes, value_topics);
            }
        }

        let mut nodes = 0;
        let mut value_topics = 0;
        walk(&self.root, &mut nodes, &mut value_topics);
        self.node_count = nodes;
        self.value_topic_count = value_topics;
    }
}

/// Calls `f` for every path prefix (root to leaf) of `topic`.
///
/// The synthetic group node for topics with a leading slash is reported as
/// [`LEADING_SLASH_GROUP`], because its MQTT path would be empty.
pub fn for_each_path_prefix(topic: &str, mut f: impl FnMut(&str)) {
    let mut path = String::with_capacity(topic.len());
    for (depth, segment) in topic.split('/').enumerate() {
        if depth > 0 {
            path.push('/');
        }
        path.push_str(segment);
        if depth == 0 && segment.is_empty() {
            f(LEADING_SLASH_GROUP);
        } else {
            f(&path);
        }
    }
}

/// Inserts every path prefix (root to leaf) of `topic` into `paths`.
///
/// This is used to remember which expand/collapse triangles lead to an
/// updated topic.
pub fn insert_path_prefixes(topic: &str, paths: &mut HashSet<String>) {
    for_each_path_prefix(topic, |path| {
        paths.insert(path.to_owned());
    });
}

fn prune_empty(node: &mut TopicNode) {
    for child in node.children.values_mut() {
        prune_empty(child);
    }
    node.children
        .retain(|_, child| child.data.is_some() || !child.children.is_empty());
}

fn push_visible<'a>(node: &'a TopicNode, depth: usize, rows: &mut Vec<TopicRow<'a>>) {
    rows.push(row(node, depth, node.expanded));
    if node.expanded {
        for child in node.children.values() {
            push_visible(child, depth + 1, rows);
        }
    }
}

fn collect_filtered<'a>(
    node: &'a TopicNode,
    depth: usize,
    needle: &str,
    search_payload: bool,
    since: Option<SinceFilter>,
    rows: &mut Vec<TopicRow<'a>>,
) -> bool {
    if rows.len() >= MAX_VISIBLE_ROWS {
        return false;
    }

    let mut child_rows = Vec::new();
    let mut child_matches = false;
    for child in node.children.values() {
        if collect_filtered(
            child,
            depth + 1,
            needle,
            search_payload,
            since,
            &mut child_rows,
        ) {
            child_matches = true;
        }
    }

    let payload_matches = search_payload
        && !needle.is_empty()
        && node.data.as_ref().is_some_and(|data| {
            data.latest().is_some_and(|message| {
                message
                    .text()
                    .is_some_and(|text| text.to_lowercase().contains(needle))
            })
        });
    let text_matches = needle.is_empty()
        || display_path(&node.path).to_lowercase().contains(needle)
        || payload_matches;
    let time_matches =
        since.is_none_or(|filter| node.data.as_ref().is_some_and(|data| filter.matches(data)));
    let self_matches = text_matches && time_matches;

    if self_matches || child_matches {
        // Filtered rows respect the user's expand/collapse state so branches
        // can still be folded while a filter is active.
        rows.push(row(node, depth, node.expanded));
        if node.expanded {
            rows.extend(child_rows);
        }
        true
    } else {
        false
    }
}

fn row<'a>(node: &'a TopicNode, depth: usize, expanded: bool) -> TopicRow<'a> {
    let (total, stored, preview, retained) = match &node.data {
        Some(data) => (
            data.total,
            data.history.len(),
            data.preview.as_str(),
            data.latest().is_some_and(|message| message.retain),
        ),
        None => (0, 0, "", false),
    };

    TopicRow {
        // Rows address nodes by path. The leading-slash group needs a key of
        // its own because its path is empty, just like the invisible root's.
        path: if node.path.is_empty() {
            LEADING_SLASH_GROUP
        } else {
            &node.path
        },
        segment: &node.segment,
        depth,
        has_children: node.has_children(),
        expanded,
        total,
        stored,
        preview,
        retained,
        has_data: node.data.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(topic: &str, payload: &[u8], retain: bool) -> MessageRecord {
        MessageRecord::new(topic.to_owned(), payload.to_vec(), 0, retain)
    }

    fn message_at(topic: &str, received_at: DateTime<Local>) -> MessageRecord {
        let mut message = message(topic, b"1", false);
        message.received_at = received_at;
        message
    }

    #[test]
    fn inserting_builds_tree_and_counts() {
        let mut tree = TopicTree::default();
        assert_eq!(
            tree.insert(message("home/kitchen/temperature", b"21", false), 100),
            InsertOutcome::NewTopic
        );
        assert_eq!(
            tree.insert(message("home/kitchen/humidity", b"55", false), 100),
            InsertOutcome::NewTopic
        );
        assert_eq!(
            tree.insert(message("home/kitchen/temperature", b"22", false), 100),
            InsertOutcome::Updated
        );

        assert_eq!(tree.value_topic_count, 2);
        assert_eq!(tree.node_count, 4); // home, kitchen, temperature, humidity
        assert_eq!(tree.received_messages, 3);
        assert_eq!(tree.data("home/kitchen/temperature").unwrap().total, 2);
        assert_eq!(
            tree.data("home/kitchen/temperature")
                .unwrap()
                .latest()
                .unwrap()
                .text(),
            Some("22")
        );
    }

    #[test]
    fn history_is_capped() {
        let mut tree = TopicTree::default();
        for value in 0..50 {
            tree.insert(message("a/b", value.to_string().as_bytes(), false), 10);
        }
        let data = tree.data("a/b").unwrap();
        assert_eq!(data.history.len(), 10);
        assert_eq!(data.total, 50);
    }

    #[test]
    fn empty_retained_payload_removes_topic() {
        let mut tree = TopicTree::default();
        tree.insert(message("a/b", b"value", true), 100);
        assert!(tree.data("a/b").is_some());

        assert_eq!(
            tree.insert(message("a/b", b"", true), 100),
            InsertOutcome::Removed
        );
        assert!(tree.data("a/b").is_none());
        assert!(tree.node("a").is_none());
    }

    #[test]
    fn retained_topics_under_prefix_skips_live_topics() {
        let mut tree = TopicTree::default();
        tree.insert(message("home/kitchen/temperature", b"21", true), 100);
        tree.insert(message("home/kitchen/humidity", b"55", false), 100);
        tree.insert(message("home/livingroom/temperature", b"22", true), 100);
        tree.insert(message("garden/temperature", b"23", true), 100);

        assert_eq!(
            tree.retained_topics_under("home"),
            vec![
                "home/kitchen/temperature".to_owned(),
                "home/livingroom/temperature".to_owned()
            ]
        );
        assert_eq!(
            tree.retained_topics_under("home/kitchen"),
            vec!["home/kitchen/temperature".to_owned()]
        );
        assert!(tree.retained_topics_under("home/bathroom").is_empty());
        assert_eq!(
            tree.retained_topics_under(""),
            vec![
                "garden/temperature".to_owned(),
                "home/kitchen/temperature".to_owned(),
                "home/livingroom/temperature".to_owned()
            ]
        );
    }

    #[test]
    fn retained_topics_under_the_slash_group_ignore_other_topics() {
        let mut tree = TopicTree::default();
        tree.insert(message("/home/kitchen/temperature", b"21", true), 100);
        tree.insert(message("home/kitchen/temperature", b"22", true), 100);

        assert_eq!(
            tree.retained_topics_under(LEADING_SLASH_GROUP),
            vec!["/home/kitchen/temperature".to_owned()]
        );
    }

    #[test]
    fn filter_keeps_ancestors_of_matches() {
        let mut tree = TopicTree::default();
        tree.insert(message("home/kitchen/temperature", b"21", false), 100);
        tree.insert(message("home/livingroom/light", b"on", false), 100);
        tree.set_all_expanded(true);

        let rows = tree.visible_rows("kitchen", false, None);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert!(paths.contains(&"home"));
        assert!(paths.contains(&"home/kitchen"));
        assert!(paths.contains(&"home/kitchen/temperature"));
        assert!(!paths.contains(&"home/livingroom"));
    }

    #[test]
    fn filtered_rows_respect_collapsed_branches() {
        let mut tree = TopicTree::default();
        tree.insert(message("home/kitchen/temperature", b"21", false), 100);
        tree.set_all_expanded(true);

        tree.toggle_expanded("home");
        let rows = tree.visible_rows("kitchen", false, None);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert_eq!(paths, vec!["home"]);
        assert!(!rows[0].expanded);

        tree.toggle_expanded("home");
        let rows = tree.visible_rows("kitchen", false, None);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert_eq!(
            paths,
            vec!["home", "home/kitchen", "home/kitchen/temperature"]
        );
    }

    #[test]
    fn collapse_all_hides_children() {
        let mut tree = TopicTree::default();
        tree.insert(message("a/b/c", b"1", false), 100);
        tree.set_all_expanded(true);
        let rows_before = tree.visible_rows("", false, None).len();
        assert_eq!(rows_before, 3);

        tree.set_all_expanded(false);
        let rows_after = tree.visible_rows("", false, None).len();
        assert_eq!(rows_after, 1);
    }

    #[test]
    fn new_nodes_start_collapsed() {
        let mut tree = TopicTree::default();
        tree.insert(message("a/b", b"1", false), 100);

        let rows = tree.visible_rows("", false, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "a");
        assert!(!rows[0].expanded);
    }

    #[test]
    fn remove_topic_prunes_empty_ancestors() {
        let mut tree = TopicTree::default();
        tree.insert(message("home/kitchen/temperature", b"21", false), 100);
        assert_eq!(tree.node_count, 3);

        assert!(tree.remove_topic("home/kitchen/temperature"));
        assert_eq!(tree.node_count, 0);
        assert_eq!(tree.value_topic_count, 0);
        assert!(tree.node("home").is_none());
        assert!(tree.visible_rows("", false, None).is_empty());
    }

    #[test]
    fn payload_search_uses_the_full_latest_payload() {
        let mut tree = TopicTree::default();
        let payload = format!("{}needle", "x".repeat(200));
        tree.insert(message("long/payload", payload.as_bytes(), false), 100);
        tree.set_all_expanded(true);

        let rows = tree.visible_rows("needle", true, None);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert!(paths.contains(&"long/payload"));
    }

    #[test]
    fn leading_slash_topics_hang_below_a_slash_group() {
        let mut tree = TopicTree::default();
        tree.insert(message("/home/kitchen", b"21", false), 100);

        assert!(tree.node("/home/kitchen").is_some());
        assert_eq!(
            tree.data("/home/kitchen").unwrap().latest().unwrap().text(),
            Some("21")
        );
        assert!(tree.node(LEADING_SLASH_GROUP).is_some());

        tree.set_all_expanded(true);
        let rows = tree.visible_rows("", false, None);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert_eq!(
            paths,
            vec![LEADING_SLASH_GROUP, "/home", "/home/kitchen"],
            "leading-slash topics are children of the / group"
        );
        // The group's empty segment is rendered as "/".
        assert_eq!(rows[0].segment, "");
        assert_eq!(display_path(rows[0].path), "/");
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].depth, 2);
    }

    #[test]
    fn leading_slash_group_collapses_like_any_branch() {
        let mut tree = TopicTree::default();
        tree.insert(message("/home/kitchen", b"21", false), 100);
        tree.set_all_expanded(true);

        tree.toggle_expanded(LEADING_SLASH_GROUP);
        let rows = tree.visible_rows("", false, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, LEADING_SLASH_GROUP);
        assert!(!rows[0].expanded);
    }

    #[test]
    fn leading_slash_group_is_kept_while_filtering() {
        let mut tree = TopicTree::default();
        tree.insert(message("/home/kitchen/temperature", b"21", false), 100);
        tree.set_all_expanded(true);

        let rows = tree.visible_rows("kitchen", false, None);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert_eq!(
            paths,
            vec![
                LEADING_SLASH_GROUP,
                "/home",
                "/home/kitchen",
                "/home/kitchen/temperature"
            ]
        );
        assert_eq!(rows[0].depth, 0);

        // Without a matching child the group does not appear either.
        assert!(tree.visible_rows("garden", false, None).is_empty());
    }

    #[test]
    fn removing_the_leading_slash_group_keeps_normal_topics() {
        let mut tree = TopicTree::default();
        tree.insert(message("/home/kitchen", b"21", false), 100);
        tree.insert(message("home/kitchen", b"22", false), 100);

        assert!(tree.remove_topic(LEADING_SLASH_GROUP));
        assert!(tree.node(LEADING_SLASH_GROUP).is_none());
        assert!(tree.node("/home").is_none());
        assert!(tree.data("/home/kitchen").is_none());
        assert_eq!(
            tree.data("home/kitchen").unwrap().latest().unwrap().text(),
            Some("22")
        );
        assert_eq!(tree.value_topic_count, 1);
    }

    #[test]
    fn since_filter_keeps_only_recent_topics() {
        let now = Local::now();
        let mut tree = TopicTree::default();
        tree.insert(
            message_at("old/topic", now - chrono::Duration::hours(2)),
            100,
        );
        tree.insert(
            message_at("new/topic", now - chrono::Duration::minutes(5)),
            100,
        );
        tree.set_all_expanded(true);

        let since = Some(SinceFilter {
            mode: SinceMode::ReceivedSince,
            cutoff: now - chrono::Duration::minutes(15),
        });
        let rows = tree.visible_rows("", false, since);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();

        assert!(paths.contains(&"new"));
        assert!(paths.contains(&"new/topic"));
        assert!(!paths.contains(&"old"));
        assert!(!paths.contains(&"old/topic"));
    }

    #[test]
    fn since_filtered_branches_can_still_be_collapsed() {
        let now = Local::now();
        let mut tree = TopicTree::default();
        tree.insert(
            message_at("new/topic", now - chrono::Duration::minutes(5)),
            100,
        );
        tree.set_all_expanded(true);

        let since = Some(SinceFilter {
            mode: SinceMode::ReceivedSince,
            cutoff: now - chrono::Duration::minutes(15),
        });

        tree.toggle_expanded("new");
        let rows = tree.visible_rows("", false, since);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert_eq!(paths, vec!["new"]);
        assert!(!rows[0].expanded);

        tree.toggle_expanded("new");
        let rows = tree.visible_rows("", false, since);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert_eq!(paths, vec!["new", "new/topic"]);
    }

    #[test]
    fn retained_backlog_does_not_count_as_live_activity() {
        let now = Local::now();
        let mut tree = TopicTree::default();
        let mut message = message_at("lwt/topic", now - chrono::Duration::minutes(1));
        message.retain = true;
        message.backlog = true;
        tree.insert(message, 100);
        tree.set_all_expanded(true);

        let received_since = Some(SinceFilter {
            mode: SinceMode::ReceivedSince,
            cutoff: now - chrono::Duration::minutes(15),
        });
        assert!(tree.visible_rows("", false, received_since).is_empty());

        let no_messages_since = Some(SinceFilter {
            mode: SinceMode::NoMessagesSince,
            cutoff: now - chrono::Duration::minutes(15),
        });
        let rows = tree.visible_rows("", false, no_messages_since);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert_eq!(paths, vec!["lwt", "lwt/topic"]);
    }

    #[test]
    fn live_messages_after_backlog_are_activity() {
        let now = Local::now();
        let mut tree = TopicTree::default();
        let mut backlog = message_at("lwt/topic", now - chrono::Duration::minutes(1));
        backlog.retain = true;
        backlog.backlog = true;
        tree.insert(backlog, 100);
        tree.insert(
            message_at("lwt/topic", now - chrono::Duration::minutes(1)),
            100,
        );
        tree.set_all_expanded(true);

        let since = Some(SinceFilter {
            mode: SinceMode::ReceivedSince,
            cutoff: now - chrono::Duration::minutes(15),
        });
        let rows = tree.visible_rows("", false, since);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();
        assert_eq!(paths, vec!["lwt", "lwt/topic"]);
    }

    #[test]
    fn no_messages_since_filter_selects_the_old_topics() {
        let now = Local::now();
        let mut tree = TopicTree::default();
        tree.insert(
            message_at("old/topic", now - chrono::Duration::hours(2)),
            100,
        );
        tree.insert(
            message_at("new/topic", now - chrono::Duration::minutes(5)),
            100,
        );

        tree.set_all_expanded(true);

        let since = Some(SinceFilter {
            mode: SinceMode::NoMessagesSince,
            cutoff: now - chrono::Duration::minutes(15),
        });
        let rows = tree.visible_rows("", false, since);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();

        assert!(paths.contains(&"old"));
        assert!(paths.contains(&"old/topic"));
        assert!(!paths.contains(&"new"));
        assert!(!paths.contains(&"new/topic"));
    }

    #[test]
    fn since_filter_combines_with_the_text_filter() {
        let now = Local::now();
        let mut tree = TopicTree::default();
        tree.insert(
            message_at(
                "home/kitchen/temperature",
                now - chrono::Duration::minutes(5),
            ),
            100,
        );
        tree.insert(
            message_at(
                "home/garden/temperature",
                now - chrono::Duration::minutes(5),
            ),
            100,
        );
        tree.insert(
            message_at("home/kitchen/humidity", now - chrono::Duration::hours(2)),
            100,
        );

        tree.set_all_expanded(true);

        let since = Some(SinceFilter {
            mode: SinceMode::ReceivedSince,
            cutoff: now - chrono::Duration::minutes(15),
        });
        let rows = tree.visible_rows("kitchen", false, since);
        let paths: Vec<&str> = rows.iter().map(|row| row.path).collect();

        assert!(paths.contains(&"home"));
        assert!(paths.contains(&"home/kitchen"));
        assert!(paths.contains(&"home/kitchen/temperature"));
        assert!(!paths.contains(&"home/kitchen/humidity"));
        assert!(!paths.contains(&"home/garden"));
    }

    #[test]
    fn since_filter_keeps_ancestors_of_matches() {
        let now = Local::now();
        let mut tree = TopicTree::default();
        tree.insert(
            message_at(
                "home/kitchen/temperature",
                now - chrono::Duration::minutes(5),
            ),
            100,
        );

        tree.set_all_expanded(true);

        let since = Some(SinceFilter {
            mode: SinceMode::ReceivedSince,
            cutoff: now - chrono::Duration::minutes(15),
        });
        let rows = tree.visible_rows("", false, since);

        // `home` and `home/kitchen` have no messages of their own but are
        // kept as ancestors of the matching leaf.
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].path, "home");
        assert_eq!(rows[0].total, 0);
    }

    #[test]
    fn path_prefixes_include_every_ancestor() {
        let mut paths = HashSet::new();
        insert_path_prefixes("esp32/airquality/sensor", &mut paths);
        assert_eq!(
            paths,
            HashSet::from([
                "esp32".to_owned(),
                "esp32/airquality".to_owned(),
                "esp32/airquality/sensor".to_owned(),
            ])
        );

        let mut paths = HashSet::new();
        insert_path_prefixes("/esp32/sensor", &mut paths);
        assert_eq!(
            paths,
            HashSet::from([
                LEADING_SLASH_GROUP.to_owned(),
                "/esp32".to_owned(),
                "/esp32/sensor".to_owned(),
            ])
        );
    }
}
