use anyhow::{Result, Context};
use async_trait::async_trait;
use futures::StreamExt;
use twilight_gateway::{EventType, Event};
use twilight_model::{gateway::payload::{incoming::InteractionCreate, outgoing::UpdateVoiceState}, application::interaction::{application_command::CommandOptionValue, InteractionData}};

use crate::{try_unpack, State, interaction_response, get_option_as, player::Player, reply, update_reply, voice::connect_voice_gateway};

use super::CommandHandler;

pub struct JoinCommand;

#[async_trait]
impl CommandHandler for JoinCommand {
  async fn run(&self, state: State, interaction: Box<InteractionCreate>) -> Result<()> {
    reply!(state, interaction, &interaction_response!(
      DeferredChannelMessageWithSource,
      content("Joining...")
    )).await?;

    let command = try_unpack!(interaction.data.as_ref().context("no interaction data")?, InteractionData::ApplicationCommand)?;
    let guild_id = interaction.guild_id.unwrap();
    let voice_state = state.cache.voice_state(interaction.member.as_ref().unwrap().user.as_ref().unwrap().id, guild_id);
    let channel_id = get_option_as!(command, "channel", CommandOptionValue::Channel)
      .map(|it| *it.unwrap())
      .or(voice_state.map(|it| it.channel_id()))
      .unwrap();
    let user_id = state.cache.current_user().unwrap().id;

    state.sender.command(&UpdateVoiceState::new(guild_id, channel_id, true, false))?;
    println!("Send");

    let mut voice_state = None;
    let mut voice_server = None;

    let mut stream = state.standby.wait_for_stream(guild_id, |event: &Event| {
      match event.kind() {
        EventType::VoiceStateUpdate => true,
        EventType::VoiceServerUpdate => true,
        _ => false
      }
    });

    while let Some(event) = stream.next().await {
      println!("get {:?}", event);
      match event {
        Event::VoiceStateUpdate(vs) => {
          voice_state = Some(vs);
        }
        Event::VoiceServerUpdate(vs) => {
          voice_server = Some(vs);
        }
        _ => {}
      }

      if voice_state.is_some() && voice_server.is_some() {
        break;
      }
    }

    let voice_state_update = voice_state.unwrap();
    let voice_server_update = voice_server.unwrap();

    println!("{:?} {:?}", voice_state_update, voice_server_update);

    println!("connecting");
    connect_voice_gateway(
      &voice_server_update.endpoint.unwrap(),
      voice_server_update.guild_id.get(),
      user_id.get(),
      &voice_state_update.session_id,
      &voice_server_update.token
    ).await?;

    // let call = state.songbird.join(guild_id, channel_id).await?;

    // let mut player = Player::new(state.clone(), guild_id);
    // player.channel_id = Some(channel_id);
    // player.call = Some(call);
    // state.players.write().await.insert(guild_id, player);

    update_reply!(state, interaction)
      .content(Some(&format!("Joined channel <#{}>", channel_id)))?
      .await?;

    Ok(())
  }
}
