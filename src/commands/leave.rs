use crate::{
    errors::ParrotError, guild::stored_queue::GuildStoredQueueMap,
    messaging::message::ParrotMessage, utils::create_response,
};
use serenity::{
    client::Context,
    model::application::interaction::application_command::ApplicationCommandInteraction,
};

pub async fn leave(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), ParrotError> {
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let mut data = ctx.data.write().await;
    match data.get_mut::<GuildStoredQueueMap>() {
        Some(stored_queue) => match stored_queue.get_mut(&guild_id) {
            Some(guild_stored_queue) => {
                guild_stored_queue.queue.clear();
            }
            None => (),
        },
        _ => (),
    }

    drop(data);

    manager.remove(guild_id).await.unwrap();

    create_response(&ctx.http, interaction, ParrotMessage::Leaving).await
}
