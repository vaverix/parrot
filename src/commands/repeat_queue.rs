use serenity::{all::CommandInteraction, client::Context};

use crate::{
    errors::ParrotError,
    guild::settings::{GuildSettings, GuildSettingsMap},
    messaging::message::ParrotMessage,
    utils::create_response,
};

pub async fn repeat_queue(
    ctx: &Context,
    interaction: &mut CommandInteraction,
) -> Result<(), ParrotError> {
    let guild_id = interaction.guild_id.unwrap();
    let mut data = ctx.data.write().await;
    let settings = data.get_mut::<GuildSettingsMap>().unwrap();

    let guild_setting = settings
        .entry(guild_id)
        .or_insert_with(|| GuildSettings::new(guild_id));
    guild_setting.toggle_queue_loop();
    guild_setting.save()?;
    let is_queue_loop = guild_setting.queue_loop;

    drop(data);

    if is_queue_loop {
        create_response(&ctx.http, interaction, ParrotMessage::LoopEnable).await
    } else {
        create_response(&ctx.http, interaction, ParrotMessage::LoopDisable).await
    }
}
