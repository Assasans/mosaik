use crate::{include_and_export, AnyError, PoiseContext};

include_and_export!(play pause filters seek queue debug jump);

/// Show this help menu
#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn help(
  ctx: PoiseContext<'_>,
  #[description = "Specific command to show help about"]
  #[autocomplete = "poise::builtins::autocomplete_command"]
  command: Option<String>
) -> Result<(), AnyError> {
  poise::builtins::help(ctx, command.as_deref(), poise::builtins::HelpConfiguration {
    extra_text_at_bottom: "This is an example bot made to showcase features of my custom Discord bot framework",
    ..Default::default()
  })
  .await?;
  Ok(())
}
