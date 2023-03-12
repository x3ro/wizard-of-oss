use std::collections::HashMap;

use anyhow::{anyhow, format_err};
use slack_morphism::prelude::*;
use tracing::error;

use crate::models::OpenSourceAttachment;
use crate::{AppConfig, AppState};

fn cmp_block_id(block_id: &Option<SlackBlockId>, expected: impl AsRef<str>) -> bool {
    let expected = expected.as_ref();
    block_id
        .as_ref()
        .map(|actual| actual.0.eq(expected))
        .unwrap_or(false)
}

fn get_block(view: &mut SlackModalView, block_id: impl AsRef<str>) -> Option<&mut SlackBlock> {
    let block_id = block_id.as_ref();
    view.blocks.iter_mut().find(|block| match block {
        SlackBlock::Section(x) => cmp_block_id(&x.block_id, block_id),
        SlackBlock::Header(x) => cmp_block_id(&x.block_id, block_id),
        SlackBlock::Divider(x) => cmp_block_id(&x.block_id, block_id),
        SlackBlock::Image(x) => cmp_block_id(&x.block_id, block_id),
        SlackBlock::Actions(x) => cmp_block_id(&x.block_id, block_id),
        SlackBlock::Context(x) => cmp_block_id(&x.block_id, block_id),
        SlackBlock::Input(x) => cmp_block_id(&x.block_id, block_id),
        SlackBlock::File(x) => cmp_block_id(&x.block_id, block_id),
        SlackBlock::RichText(_) => false,
    })
}

const RECORD_HOURS_MODAL: &str = include_str!("../slack-ui/modal.json");
pub async fn open_oss_modal(
    state: &AppState,
    trigger_id: SlackTriggerId,
    default_country: Option<String>,
) -> anyhow::Result<()> {
    // TODO: Would be nice if we caught it on startup if deserialization
    //       of this view fails.
    let mut modal: SlackModalView = serde_json::from_str(RECORD_HOURS_MODAL).unwrap();

    let block = get_block(&mut modal, "country").expect("Modal is missing `country` field");

    if let Some(default_country) = default_country {
        match block {
            SlackBlock::Input(SlackInputBlock {
                element:
                    SlackInputBlockElement::StaticSelect(
                        SlackBlockStaticSelectElement {
                            options: Some(options),
                            initial_option,
                            ..
                        },
                        ..,
                    ),
                ..
            }) => {
                let preselected = options.iter().find(|opt| opt.value == default_country);
                *initial_option = preselected.cloned();
            }
            _ => {
                error!("Tried to set the default value for the open source modal, but structure of the modal was not as expected");
            }
        }
    }

    let req = SlackApiViewsOpenRequest {
        trigger_id,
        view: SlackView::Modal(modal),
    };

    state.get_session().views_open(&req).await.unwrap();

    Ok(())
}

pub async fn report_user_stats(state: &AppState, config: &AppConfig, event: &SlackCommandEvent) {
    let req = SlackApiConversationsHistoryRequest {
        channel: Some(SlackChannelId(config.slack_oss_channel_id.clone())),
        cursor: None,
        latest: None,
        // TODO: Do we need to implement pagination here?
        //       Take a look at how the current application handles it.
        limit: Some(100),
        oldest: None,
        inclusive: None,
    };

    let res = state
        .get_session()
        .conversations_history(&req)
        .await
        .unwrap();

    let mut hours: HashMap<String, i16> = HashMap::new();

    for x in &res.messages {
        let Some(attachments) = &x.content.attachments else {
            continue;
        };

        let entries: Vec<anyhow::Result<OpenSourceAttachment>> = attachments
            .iter()
            .map(|x| x.fields.clone())
            .map(|x| {
                if let Some(fields) = x {
                    fields.try_into()
                } else {
                    Err(format_err!("No attachments"))
                }
            })
            .collect();

        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };

            let current = hours.get(&entry.username).unwrap_or(&0);
            hours.insert(entry.username.to_string(), current + entry.number_of_hours);
        }
    }

    let req = SlackApiChatPostEphemeralRequest {
        channel: SlackChannelId(config.slack_oss_channel_id.clone()),
        user: event.user_id.clone(),
        content: SlackMessageContent::new().with_text(format!("{:#?}", hours)),
        as_user: None,
        icon_emoji: None,
        icon_url: None,
        link_names: None,
        parse: None,
        thread_ts: None,
        username: None,
    };

    state.get_session().chat_post_ephemeral(&req).await.unwrap();
}

pub trait SlackViewStateExt {
    fn input_value(&self, name: impl AsRef<str>) -> anyhow::Result<String>;
    fn select_value(&self, name: impl AsRef<str>) -> anyhow::Result<String>;
}

impl SlackViewStateExt for SlackViewState {
    /// From the slack view state (received when a view is submitted), this function
    /// tries to extract the value for the field with the given name.
    /// See <https://api.slack.com/reference/interaction-payloads/views> for more details.
    fn input_value(&self, name: impl AsRef<str>) -> anyhow::Result<String> {
        let id = name.as_ref();
        self.values
            .get(&id.into())
            .and_then(|x| x.get(&id.into()))
            .and_then(|x| x.value.to_owned())
            .ok_or_else(|| anyhow!("Missing field '{}'", name.as_ref()))
    }

    /// Same as [`SlackViewStateExt::input_value`], but for a select field
    fn select_value(&self, name: impl AsRef<str>) -> anyhow::Result<String> {
        let id = name.as_ref();
        self.values
            .get(&id.into())
            .and_then(|x| x.get(&id.into()))
            .and_then(|x| x.selected_option.as_ref())
            .map(|x| x.value.clone())
            .ok_or_else(|| anyhow!("Missing select '{}'", name.as_ref()))
    }
}
