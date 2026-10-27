//! Durable Narrator inference-to-Chat completion contract tests.

use rintawa_chat::{
    ChatConversationSummary, ChatConversationView, ChatMessageView, ChatParticipantView,
    ContentBlock, MessageAddedEvent, SendMessageCommand,
};
use rintawa_narrator::{
    ControllerMode, ControllerPolicy, DurableCompletionIdentity, DurableReaction,
    DurableReplyError, ReactionIntent, complete_durable_turn, prepare_durable_turn,
};
use rintawa_sdk::{
    world::{CorrelationId, EffectJobId, EntityId, PrincipalId, WorldEventId, WorldId},
    world_system::{WorldSystemActor, WorldSystemCausation},
};

struct Fixture {
    world: WorldId,
    principal: PrincipalId,
    conversation: EntityId,
    responder: EntityId,
    responder_entity: EntityId,
    source_event: WorldEventId,
    job: EffectJobId,
    user_message: EntityId,
    revision: EntityId,
    policy: ControllerPolicy,
    reaction: DurableReaction,
    view: ChatConversationView,
}

impl Fixture {
    fn new() -> Self {
        let world = WorldId::new();
        let principal = PrincipalId::new();
        let conversation = EntityId::new();
        let responder = EntityId::new();
        let responder_entity = EntityId::new();
        let user = EntityId::new();
        let greeting = EntityId::new();
        let user_message = EntityId::new();
        let revision = EntityId::new();
        let source_event = WorldEventId::new();
        let message = MessageAddedEvent {
            conversation_id: conversation,
            message_id: user_message,
            revision_id: revision,
            participant_id: user,
            parent_message_id: Some(greeting),
            alternative_of: None,
        };
        let reaction = DurableReaction {
            intent: ReactionIntent::from_committed_message(world, source_event, 4, &message)
                .expect("source"),
            responder_participant_id: responder,
            responder_entity_id: responder_entity,
            controller_policy_revision: 7,
        };
        let policy = ControllerPolicy::new(
            world,
            7,
            ControllerMode::Explicit {
                conversation_id: conversation,
                participant_id: responder,
            },
        )
        .expect("policy");
        let view = ChatConversationView {
            conversations: vec![ChatConversationSummary {
                id: conversation,
                title: Some(String::from("Forest")),
            }],
            selected_conversation_id: Some(conversation),
            selected_leaf: Some(user_message),
            participants: vec![
                ChatParticipantView {
                    id: responder,
                    display_name: String::from("NPC"),
                    linked_entity: Some(responder_entity),
                    can_send: true,
                },
                ChatParticipantView {
                    id: user,
                    display_name: String::from("Player"),
                    linked_entity: None,
                    can_send: true,
                },
            ],
            messages: vec![
                ChatMessageView {
                    id: greeting,
                    author_participant_id: responder,
                    parent_message_id: None,
                    alternative_of: None,
                    revision_id: EntityId::new(),
                    blocks: vec![ContentBlock::Markdown {
                        markdown: String::from("Welcome."),
                    }],
                    effective_at: None,
                },
                ChatMessageView {
                    id: user_message,
                    author_participant_id: user,
                    parent_message_id: Some(greeting),
                    alternative_of: None,
                    revision_id: revision,
                    blocks: vec![ContentBlock::Text {
                        text: String::from("Hello."),
                    }],
                    effective_at: None,
                },
            ],
        };
        Self {
            world,
            principal,
            conversation,
            responder,
            responder_entity,
            source_event,
            job: EffectJobId::new(),
            user_message,
            revision,
            policy,
            reaction,
            view,
        }
    }

    fn identity(&self) -> DurableCompletionIdentity {
        DurableCompletionIdentity {
            world_id: self.world,
            job_id: self.job,
            principal: self.principal,
            correlation_id: CorrelationId::new(),
        }
    }

    fn complete(
        &self,
        position: u64,
    ) -> Result<rintawa_sdk::world_system::WorldSystemCommand, DurableReplyError> {
        complete_durable_turn(
            self.identity(),
            &self.reaction,
            &self.policy,
            &self.view,
            position,
            "Hello, human.",
        )
    }
}

#[test]
fn test_should_build_only_one_effect_fenced_chat_command_from_exact_fresh_branch() {
    let scenario = Fixture::new();
    let prepared = prepare_durable_turn(
        &scenario.reaction,
        &scenario.policy,
        &scenario.view,
        12,
        scenario.world,
    )
    .expect("pinned turn");
    assert_eq!(prepared.anchor.trigger_message_id, scenario.user_message);
    assert_eq!(prepared.anchor.trigger_revision_id, scenario.revision);
    assert_eq!(
        prepared.anchor.responder_entity_id,
        scenario.responder_entity
    );
    let command = scenario.complete(12).expect("valid reply");
    assert_eq!(command.id.into_bytes(), scenario.job.into_bytes());
    assert_eq!(command.principal, scenario.principal);
    assert_eq!(
        command.actor,
        WorldSystemActor::Entity(scenario.responder_entity)
    );
    assert_eq!(
        command.causation,
        Some(WorldSystemCausation::Effect(scenario.job))
    );
    assert_eq!(command.expected_position, Some(12));
    assert_eq!(command.effective_at, None);
    let reply: SendMessageCommand =
        serde_json::from_value(command.payload.clone()).expect("Chat-owned command");
    assert_eq!(reply.conversation_id, scenario.conversation);
    assert_eq!(reply.participant_id, scenario.responder);
    assert_eq!(reply.parent_message_id, Some(scenario.user_message));
    assert_eq!(reply.alternative_of, None);
    assert_eq!(
        reply.blocks,
        vec![ContentBlock::Markdown {
            markdown: String::from("Hello, human.")
        },]
    );

    let encoded = serde_json::to_vec(&command).expect("World command");
    let decoded: rintawa_sdk::world_system::WorldSystemCommand =
        serde_json::from_slice(&encoded).expect("canonical codec");
    assert_eq!(decoded, command);
    assert!(
        !String::from_utf8(encoded)
            .expect("UTF-8")
            .contains("api_key")
    );
    assert_ne!(
        scenario.source_event.into_bytes(),
        scenario.job.into_bytes()
    );
}

#[test]
fn test_should_reject_stale_revision_branch_handoff_and_missing_grant() {
    let mut scenario = Fixture::new();
    scenario.view.messages[1].revision_id = EntityId::new();
    assert_eq!(scenario.complete(12), Err(DurableReplyError::StaleChat));

    scenario = Fixture::new();
    scenario.view.selected_leaf = scenario.view.messages.first().map(|entry| entry.id);
    assert_eq!(scenario.complete(12), Err(DurableReplyError::StaleChat));

    scenario = Fixture::new();
    scenario.view.messages[1].parent_message_id = None;
    assert_eq!(scenario.complete(12), Err(DurableReplyError::StaleChat));

    scenario = Fixture::new();
    scenario.view.participants[0].can_send = false;
    assert_eq!(scenario.complete(12), Err(DurableReplyError::StaleChat));

    scenario = Fixture::new();
    scenario.view.participants[0].linked_entity = Some(EntityId::new());
    assert_eq!(scenario.complete(12), Err(DurableReplyError::StaleChat));

    scenario = Fixture::new();
    scenario.view.participants[1].linked_entity = Some(EntityId::new());
    assert_eq!(scenario.complete(12), Err(DurableReplyError::StaleChat));
}

#[test]
fn test_should_reject_other_world_future_event_and_controller_change() {
    let mut scenario = Fixture::new();
    assert_eq!(scenario.complete(3), Err(DurableReplyError::WrongSource));

    scenario.reaction.controller_policy_revision = 8;
    assert_eq!(
        scenario.complete(12),
        Err(DurableReplyError::ControllerChanged)
    );
    scenario.reaction.controller_policy_revision = 7;
    assert!(scenario.complete(12).is_ok());

    let stale_policy = ControllerPolicy::new(
        scenario.world,
        8,
        ControllerMode::Explicit {
            conversation_id: scenario.conversation,
            participant_id: scenario.responder,
        },
    )
    .expect("policy");
    scenario.policy = stale_policy;
    assert_eq!(
        scenario.complete(12),
        Err(DurableReplyError::ControllerChanged)
    );

    scenario = Fixture::new();
    scenario.policy =
        ControllerPolicy::new(scenario.world, 7, ControllerMode::Disabled).expect("policy");
    assert_eq!(
        scenario.complete(12),
        Err(DurableReplyError::ControllerChanged)
    );

    scenario = Fixture::new();
    scenario.reaction.responder_participant_id = EntityId::new();
    assert_eq!(
        scenario.complete(12),
        Err(DurableReplyError::ControllerChanged)
    );

    scenario = Fixture::new();
    scenario.reaction.responder_entity_id = EntityId::new();
    assert_eq!(scenario.complete(12), Err(DurableReplyError::StaleChat));

    scenario = Fixture::new();
    let foreign_id = DurableCompletionIdentity {
        world_id: WorldId::new(),
        ..scenario.identity()
    };
    assert_eq!(
        complete_durable_turn(
            foreign_id,
            &scenario.reaction,
            &scenario.policy,
            &scenario.view,
            12,
            "hello"
        ),
        Err(DurableReplyError::WrongSource)
    );
}

#[test]
fn test_should_reject_empty_and_oversized_provider_reply_without_mutating_chat() {
    let scenario = Fixture::new();
    for text in ["", "  ", "\n\t"] {
        assert_eq!(
            complete_durable_turn(
                scenario.identity(),
                &scenario.reaction,
                &scenario.policy,
                &scenario.view,
                12,
                text
            ),
            Err(DurableReplyError::InvalidReply)
        );
    }
    assert_eq!(
        complete_durable_turn(
            scenario.identity(),
            &scenario.reaction,
            &scenario.policy,
            &scenario.view,
            12,
            &"x".repeat(40 * 1024)
        ),
        Err(DurableReplyError::InvalidReply)
    );
}
