use crate::{
    errors::ParrotError,
    guild::cache::GuildCacheMap,
    handlers::track_end::ModifyQueueHandler,
    messaging::messages::{
        QUEUE_EXPIRED, QUEUE_NOTHING_IS_PLAYING, QUEUE_NOW_PLAYING, QUEUE_NO_SONGS, QUEUE_PAGE,
        QUEUE_PAGE_OF, QUEUE_UP_NEXT,
    },
    utils::{get_human_readable_timestamp, AuxMetadataTypeMapKey},
};
use serenity::{
    all::{
        ButtonStyle, CommandInteraction, CreateActionRow, CreateEmbedFooter,
        CreateInteractionResponse, CreateInteractionResponseMessage, EditMessage,
    },
    builder::{CreateButton, CreateEmbed},
    client::Context,
    futures::StreamExt,
    model::{channel::Message, id::GuildId},
    prelude::{RwLock, TypeMap},
};
use songbird::{tracks::TrackHandle, Event, TrackEvent};
use std::{
    cmp::{max, min},
    fmt::Write,
    ops::Add,
    sync::Arc,
    time::Duration,
};

const EMBED_PAGE_SIZE: usize = 6;
const EMBED_TIMEOUT: u64 = 3600;

pub async fn queue(ctx: &Context, interaction: &mut CommandInteraction) -> Result<(), ParrotError> {
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let tracks = handler.queue().current_queue();
    drop(handler);

    let num_pages = calculate_num_pages(&tracks);
    let message = build_nav_btns(
        CreateInteractionResponseMessage::new().add_embed(create_queue_embed(&tracks, 0).await),
        0,
        num_pages,
    );

    interaction
        .create_response(&ctx.http, CreateInteractionResponse::Message(message))
        .await?;

    let mut message = interaction.get_response(&ctx.http).await?;
    let page: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));

    // store this interaction to context.data for later edits
    let mut data = ctx.data.write().await;
    let cache_map = data.get_mut::<GuildCacheMap>().unwrap();

    let cache = cache_map.entry(guild_id).or_default();
    cache.queue_messages.push((message.clone(), page.clone()));
    drop(data);

    // refresh the queue interaction whenever a track ends
    let mut handler = call.lock().await;
    handler.add_global_event(
        Event::Track(TrackEvent::End),
        ModifyQueueHandler {
            http: ctx.http.clone(),
            ctx_data: ctx.data.clone(),
            call: call.clone(),
            guild_id,
        },
    );
    drop(handler);

    let mut cib = message
        .await_component_interactions(ctx)
        .timeout(Duration::from_secs(EMBED_TIMEOUT))
        .stream();

    while let Some(mci) = cib.next().await {
        let btn_id = &mci.data.custom_id;

        // refetch the queue in case it changed
        let handler = call.lock().await;
        let tracks = handler.queue().current_queue();
        drop(handler);

        let num_pages = calculate_num_pages(&tracks);
        let mut page_wlock = page.write().await;

        *page_wlock = match btn_id.as_str() {
            "<<" => 0,
            "<" => min(page_wlock.saturating_sub(1), num_pages - 1),
            ">" => min(page_wlock.add(1), num_pages - 1),
            ">>" => num_pages - 1,
            _ => continue,
        };

        let message = build_nav_btns(
            CreateInteractionResponseMessage::new()
                .add_embed(create_queue_embed(&tracks, *page_wlock).await),
            *page_wlock,
            num_pages,
        );

        mci.create_response(&ctx, CreateInteractionResponse::UpdateMessage(message))
            .await?;
    }

    message
        .edit(
            &ctx.http,
            EditMessage::new().add_embed(CreateEmbed::new().description(QUEUE_EXPIRED)),
        )
        .await
        .unwrap();

    forget_queue_message(&ctx.data, &mut message, guild_id)
        .await
        .ok();

    Ok(())
}

pub async fn create_queue_embed(tracks: &[TrackHandle], page: usize) -> CreateEmbed {
    let mut embed = CreateEmbed::new();

    let description = if !tracks.is_empty() {
        let first_track_typemap_read_lock = tracks[0].typemap().read().await;
        let metadata = first_track_typemap_read_lock
            .get::<AuxMetadataTypeMapKey>()
            .unwrap()
            .clone();
        embed = embed.thumbnail(metadata.thumbnail.unwrap());

        format!(
            "[{}]({}) • `{}`",
            metadata.title.as_ref().unwrap(),
            metadata.source_url.as_ref().unwrap(),
            get_human_readable_timestamp(metadata.duration)
        )
    } else {
        String::from(QUEUE_NOTHING_IS_PLAYING)
    };

    embed = embed
        .field(QUEUE_NOW_PLAYING, &description, false)
        .field(QUEUE_UP_NEXT, build_queue_page(tracks, page).await, false)
        .footer(CreateEmbedFooter::new(format!(
            "{} {} {} {}",
            QUEUE_PAGE,
            page + 1,
            QUEUE_PAGE_OF,
            calculate_num_pages(tracks),
        )));

    embed
}

pub fn build_single_nav_btn(label: &str, is_disabled: bool) -> CreateButton {
    CreateButton::new(label.to_string().to_ascii_lowercase())
        .label(label)
        .style(ButtonStyle::Primary)
        .disabled(is_disabled)
        .to_owned()
}

pub fn build_nav_btns(
    message: CreateInteractionResponseMessage,
    page: usize,
    num_pages: usize,
) -> CreateInteractionResponseMessage {
    let (cant_left, cant_right) = (page < 1, page >= num_pages - 1);

    let components = vec![CreateActionRow::Buttons(vec![
        build_single_nav_btn("<<", cant_left),
        build_single_nav_btn("<", cant_left),
        build_single_nav_btn(">", cant_right),
        build_single_nav_btn(">>", cant_right),
    ])];

    message.components(components)
}

async fn build_queue_page(tracks: &[TrackHandle], page: usize) -> String {
    let start_idx = EMBED_PAGE_SIZE * page;
    let queue: Vec<&TrackHandle> = tracks
        .iter()
        .skip(start_idx + 1)
        .take(EMBED_PAGE_SIZE)
        .collect();

    if queue.is_empty() {
        return String::from(QUEUE_NO_SONGS);
    }

    let mut description = String::new();

    for (i, t) in queue.iter().enumerate() {
        let track_typemap_read_lock = t.typemap().read().await;
        let metadata = track_typemap_read_lock
            .get::<AuxMetadataTypeMapKey>()
            .unwrap()
            .clone();
        let title = metadata.title.unwrap();
        let url = metadata.source_url.unwrap();
        let duration = get_human_readable_timestamp(metadata.duration);

        let _ = writeln!(
            description,
            "`{}.` [{}]({}) • `{}`",
            i + start_idx + 1,
            title,
            url,
            duration
        );
    }

    description
}

pub fn calculate_num_pages(tracks: &[TrackHandle]) -> usize {
    let num_pages = ((tracks.len() as f64 - 1.0) / EMBED_PAGE_SIZE as f64).ceil() as usize;
    max(1, num_pages)
}

pub async fn forget_queue_message(
    data: &Arc<RwLock<TypeMap>>,
    message: &mut Message,
    guild_id: GuildId,
) -> Result<(), ()> {
    let mut data_wlock = data.write().await;
    let cache_map = data_wlock.get_mut::<GuildCacheMap>().ok_or(())?;

    let cache = cache_map.get_mut(&guild_id).ok_or(())?;
    cache.queue_messages.retain(|(m, _)| m.id != message.id);

    Ok(())
}
