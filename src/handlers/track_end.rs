use serenity::{
    async_trait,
    http::Http,
    model::id::GuildId,
    prelude::{Mutex, RwLock, TypeMap},
};
use songbird::{tracks::TrackHandle, Call, Event, EventContext, EventHandler};
use std::sync::Arc;

use crate::{
    commands::{
        play::{normal_query_type_resolver, Mode},
        queue::{build_nav_btns, calculate_num_pages, create_queue_embed, forget_queue_message},
        voteskip::forget_skip_votes,
    },
    guild::{cache::GuildCacheMap, settings::GuildSettingsMap, stored_queue::GuildStoredQueueMap},
};

pub struct TrackEndHandler {
    pub http: Arc<Http>,
    pub guild_id: GuildId,
    pub call: Arc<Mutex<Call>>,
    pub ctx_data: Arc<RwLock<TypeMap>>,
}

pub struct ModifyQueueHandler {
    pub http: Arc<Http>,
    pub ctx_data: Arc<RwLock<TypeMap>>,
    pub call: Arc<Mutex<Call>>,
    pub guild_id: GuildId,
}

#[async_trait]
impl EventHandler for TrackEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        let data_rlock = self.ctx_data.read().await;
        let settings = data_rlock.get::<GuildSettingsMap>().unwrap();
        let stored_queue = data_rlock.get::<GuildStoredQueueMap>().unwrap();

        let guild_stored_queue = stored_queue.get(&self.guild_id).unwrap();

        let (autopause, queue_loop) = settings
            .get(&self.guild_id)
            .map(|guild_settings| (guild_settings.autopause, guild_settings.queue_loop))
            .unwrap_or_default();

        if autopause {
            let handler = self.call.lock().await;
            let local_queue = handler.queue();
            local_queue.pause().ok();
        }

        if queue_loop && guild_stored_queue.continue_play {
            let handler = self.call.lock().await;
            let is_queue_empty = handler.queue().is_empty();
            drop(handler);

            if is_queue_empty {
                let stored_queue = guild_stored_queue.queue.clone();

                for item in stored_queue {
                    if let Err(err) = normal_query_type_resolver(
                        &self.call,
                        &self.http,
                        &self.ctx_data,
                        self.guild_id,
                        &item,
                        Mode::End,
                    )
                    .await
                    {
                        println!("{}", err);
                    }
                }
            }
        }

        drop(data_rlock);
        forget_skip_votes(&self.ctx_data, self.guild_id).await.ok();

        None
    }
}

#[async_trait]
impl EventHandler for ModifyQueueHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        let handler = self.call.lock().await;
        let queue = handler.queue().current_queue();
        drop(handler);

        update_queue_messages(&self.http, &self.ctx_data, &queue, self.guild_id).await;
        None
    }
}

pub async fn update_queue_messages(
    http: &Arc<Http>,
    ctx_data: &Arc<RwLock<TypeMap>>,
    tracks: &[TrackHandle],
    guild_id: GuildId,
) {
    let data = ctx_data.read().await;
    let cache_map = data.get::<GuildCacheMap>().unwrap();

    let mut messages = match cache_map.get(&guild_id) {
        Some(cache) => cache.queue_messages.clone(),
        None => return,
    };
    drop(data);

    for (message, page_lock) in messages.iter_mut() {
        // has the page size shrunk?
        let num_pages = calculate_num_pages(tracks);
        let mut page = page_lock.write().await;
        *page = usize::min(*page, num_pages - 1);

        let embed = create_queue_embed(tracks, *page);

        let edit_message = message
            .edit(&http, |edit| {
                edit.set_embed(embed);
                edit.components(|components| build_nav_btns(components, *page, num_pages))
            })
            .await;

        if edit_message.is_err() {
            forget_queue_message(ctx_data, message, guild_id).await.ok();
        };
    }
}
