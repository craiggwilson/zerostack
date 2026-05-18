//! Unit tests for subagent subsystem: ToolSet, TaskBoard, BusEvent, SubagentId.

#[cfg(test)]
mod tool_set_tests {
    use crate::agent::tools::ToolName;
    use crate::agent::toolset::ToolSet;

    /// Default includes all tools.
    #[test]
    fn default_includes_all() {
        let ts = ToolSet::default();
        assert!(ts.includes(&ToolName::READ.get()));
        assert!(ts.includes(&ToolName::WRITE.get()));
        assert!(ts.includes(&ToolName::BASH.get()));
        assert!(ts.includes(&ToolName::GREP.get()));
        assert!(ts.includes(&ToolName::FIND_FILES.get()));
        assert!(ts.includes(&ToolName::LIST_DIR.get()));
        assert!(ts.includes(&ToolName::EDIT.get()));
        assert!(ts.includes(&ToolName::WRITE_TODO_LIST.get()));
    }

    /// ReadOnly includes only read-only tools.
    #[test]
    fn read_only() {
        let ts = ToolSet::read_only();
        assert!(ts.includes(&ToolName::READ.get()));
        assert!(ts.includes(&ToolName::GREP.get()));
        assert!(ts.includes(&ToolName::FIND_FILES.get()));
        assert!(ts.includes(&ToolName::LIST_DIR.get()));
        assert!(!ts.includes(&ToolName::WRITE.get()));
        assert!(!ts.includes(&ToolName::BASH.get()));
        assert!(!ts.includes(&ToolName::EDIT.get()));
    }

    /// no_tools excludes all tools.
    #[test]
    fn no_tools_excludes_all() {
        let ts = ToolSet::no_tools();
        assert!(!ts.includes(&ToolName::READ.get()));
        assert!(!ts.includes(&ToolName::BASH.get()));
        assert!(!ts.includes(&ToolName::WRITE.get()));
    }

    /// with_tool adds a tool to a restrictive set.
    #[test]
    fn with_tool_force_include() {
        let ts = ToolSet::read_only().with_tool(ToolName::BASH.get());
        // bash is normally excluded by read_only, but with_tool forces include.
        assert!(ts.includes(&ToolName::BASH.get()));
        // read is still included.
        assert!(ts.includes(&ToolName::READ.get()));
        // write is still excluded.
        assert!(!ts.includes(&ToolName::WRITE.get()));
    }

    /// without_tool removes a tool from default set.
    #[test]
    fn without_tool_force_exclude() {
        let ts = ToolSet::default().without_tool(ToolName::BASH.get());
        assert!(!ts.includes(&ToolName::BASH.get()));
        assert!(ts.includes(&ToolName::READ.get()));
    }
}

#[cfg(test)]
mod subagent_id_tests {
    use crate::extras::subagent::SubagentId;

    #[test]
    fn display() {
        assert_eq!(SubagentId(1).to_string(), "1");
        assert_eq!(SubagentId(42).to_string(), "42");
    }

    #[test]
    fn equality() {
        assert_eq!(SubagentId(1), SubagentId(1));
        assert_ne!(SubagentId(1), SubagentId(2));
    }

    #[test]
    fn hash_in_map() {
        use std::collections::HashMap;
        let mut m: HashMap<SubagentId, &str> = HashMap::new();
        m.insert(SubagentId(1), "alice");
        m.insert(SubagentId(2), "bob");
        assert_eq!(m[&SubagentId(1)], "alice");
        assert_eq!(m[&SubagentId(2)], "bob");
    }
}

#[cfg(test)]
mod bus_event_tests {
    use crate::extras::subagent::SubagentId;
    use crate::extras::subagent::bus::BusEvent;
    use compact_str::CompactString;

    #[test]
    fn subagent_id_from_token() {
        let ev = BusEvent::Token {
            id: SubagentId(5),
            text: CompactString::from("hi"),
        };
        assert_eq!(ev.subagent_id(), SubagentId(5));
    }

    #[test]
    fn subagent_id_from_done() {
        let ev = BusEvent::Done {
            id: SubagentId(3),
            response: CompactString::from("done"),
            tokens: 100,
            cost: 0.01,
        };
        assert_eq!(ev.subagent_id(), SubagentId(3));
    }

    #[tokio::test]
    async fn relay_exits_on_channel_close() {
        // Verify that the relay task exits cleanly when event_rx closes.
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<crate::event::AgentEvent>(8);
        let (bus_tx, mut bus_rx) = tokio::sync::mpsc::channel(8);
        let handle =
            crate::extras::subagent::bus::spawn_agent_relay(SubagentId(1), event_rx, bus_tx);
        // Drop sender — relay should exit.
        drop(event_tx);
        // Give relay a moment to process the channel close.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(handle.is_finished());
        // Bus should be empty (no events emitted).
        assert!(bus_rx.try_recv().is_err());
    }

    /// Verify that a Done event flowing through the relay is correctly tagged.
    #[tokio::test]
    async fn relay_tags_done_event() {
        use crate::event::AgentEvent;
        use compact_str::CompactString;
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<AgentEvent>(8);
        let (bus_tx, mut bus_rx) = tokio::sync::mpsc::channel(8);
        let _handle =
            crate::extras::subagent::bus::spawn_agent_relay(SubagentId(7), event_rx, bus_tx);
        event_tx
            .send(AgentEvent::Done {
                response: CompactString::from("result"),
                tokens: 10,
                cost: 0.001,
            })
            .await
            .unwrap();
        drop(event_tx);
        // Give relay time to process.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let bus_ev = bus_rx.try_recv().expect("expected Done event on bus");
        assert_eq!(bus_ev.subagent_id(), SubagentId(7));
        match bus_ev {
            crate::extras::subagent::bus::BusEvent::Done { id, response, .. } => {
                assert_eq!(id, SubagentId(7));
                assert_eq!(response, "result");
            }
            other => panic!("expected Done, got {:?}", other),
        }
    }
}

/// Integration smoke test: push events directly to a relay channel and verify
/// they flow through the bus as expected. Does not require a real LLM.
#[cfg(test)]
mod integration_tests {
    use crate::event::AgentEvent;
    use crate::extras::subagent::SubagentId;
    use crate::extras::subagent::bus::{BusEvent, spawn_agent_relay};
    use compact_str::CompactString;

    /// Spawn a relay with a mock runner channel, push a Token and Done event,
    /// verify both arrive on the bus tagged with the correct SubagentId.
    #[tokio::test]
    async fn smoke_token_then_done() {
        let id = SubagentId(42);
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<AgentEvent>(16);
        let (bus_tx, mut bus_rx) = tokio::sync::mpsc::channel::<BusEvent>(16);

        let relay = spawn_agent_relay(id, event_rx, bus_tx);

        event_tx
            .send(AgentEvent::Token(CompactString::from("hello")))
            .await
            .unwrap();
        event_tx
            .send(AgentEvent::Done {
                response: CompactString::from("done response"),
                tokens: 5,
                cost: 0.0,
            })
            .await
            .unwrap();
        drop(event_tx);

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(
            relay.is_finished(),
            "relay should have exited after event_rx closed"
        );

        let ev1 = bus_rx.try_recv().expect("token event");
        assert_eq!(ev1.subagent_id(), id);
        match ev1 {
            BusEvent::Token { text, .. } => assert_eq!(text, "hello"),
            other => panic!("expected Token, got {:?}", other),
        }

        let ev2 = bus_rx.try_recv().expect("done event");
        assert_eq!(ev2.subagent_id(), id);
        match ev2 {
            BusEvent::Done { response, .. } => assert_eq!(response, "done response"),
            other => panic!("expected Done, got {:?}", other),
        }

        // Bus should be empty after the two events.
        assert!(bus_rx.try_recv().is_err(), "bus should be empty");
    }
}
