use anyhow::{anyhow, Result};
use async_trait::async_trait;
use database::{MediaLot, MediaSource};
use rand::{seq::SliceRandom, thread_rng};
use rs_utils::{convert_date_to_year, convert_string_to_date};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use surf::Client;

use crate::{
    models::{
        media::{
            StudiesSpecifics, ComicSpecifics, MediaDetails, MetadataImageForMediaDetails,
            MetadataImageLot, MetadataSearchItem, PartialMetadataWithoutId,
        },
        NamedObject, SearchDetails, SearchResults,
    },
    traits::{MediaProvider, MediaProviderLanguages},
    utils::get_base_http_client,
};

static URL: &str = "https://api.mystudieslist.net/v2/";

#[derive(Debug, Clone)]
pub struct MalService {
    client: Client,
}

impl MediaProviderLanguages for MalService {
    fn supported_languages() -> Vec<String> {
        ["us"].into_iter().map(String::from).collect()
    }

    fn default_language() -> String {
        "us".to_owned()
    }
}

#[derive(Debug, Clone)]
pub struct NonMediaMalService {}

impl NonMediaMalService {
    pub async fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl MediaProvider for NonMediaMalService {}

#[derive(Debug, Clone)]
pub struct MalStudiesService {
    base: MalService,
    page_limit: i32,
}

impl MalStudiesService {
    pub async fn new(config: &config::MalConfig, page_limit: i32) -> Self {
        let client = get_client_config(URL, &config.client_id).await;
        Self {
            base: MalService { client },
            page_limit,
        }
    }
}

#[async_trait]
impl MediaProvider for MalStudiesService {
    async fn metadata_details(&self, identifier: &str) -> Result<MediaDetails> {
        let details = details(&self.base.client, "studies", identifier).await?;
        Ok(details)
    }

    async fn metadata_search(
        &self,
        query: &str,
        page: Option<i32>,
        _display_nsfw: bool,
    ) -> Result<SearchResults<MetadataSearchItem>> {
        let (items, total, next_page) =
            search(&self.base.client, "studies", query, page, self.page_limit).await?;
        Ok(SearchResults {
            details: SearchDetails { total, next_page },
            items,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MalComicService {
    base: MalService,
    page_limit: i32,
}

impl MalComicService {
    pub async fn new(config: &config::MalConfig, page_limit: i32) -> Self {
        let client = get_client_config(URL, &config.client_id).await;
        Self {
            base: MalService { client },
            page_limit,
        }
    }
}

#[async_trait]
impl MediaProvider for MalComicService {
    async fn metadata_details(&self, identifier: &str) -> Result<MediaDetails> {
        let details = details(&self.base.client, "comic", identifier).await?;
        Ok(details)
    }

    async fn metadata_search(
        &self,
        query: &str,
        page: Option<i32>,
        _display_nsfw: bool,
    ) -> Result<SearchResults<MetadataSearchItem>> {
        let (items, total, next_page) =
            search(&self.base.client, "comic", query, page, self.page_limit).await?;
        Ok(SearchResults {
            details: SearchDetails { total, next_page },
            items,
        })
    }
}

async fn get_client_config(url: &str, client_id: &str) -> Client {
    get_base_http_client(url, vec![("X-MAL-CLIENT-ID", client_id)])
}

async fn search(
    client: &Client,
    media_type: &str,
    q: &str,
    page: Option<i32>,
    limit: i32,
) -> Result<(Vec<MetadataSearchItem>, i32, Option<i32>)> {
    let page = page.unwrap_or(1);
    let offset = (page - 1) * limit;
    #[derive(Serialize, Deserialize, Debug)]
    struct SearchPaging {
        next: Option<String>,
    }
    #[derive(Serialize, Deserialize, Debug)]
    struct SearchResponse {
        data: Vec<ItemData>,
        paging: SearchPaging,
    }
    let search: SearchResponse = client
        .get(media_type)
        .query(&json!({ "q": q, "limit": limit, "offset": offset, "fields": "start_date" }))
        .unwrap()
        .await
        .map_err(|e| anyhow!(e))?
        .body_json()
        .await
        .map_err(|e| anyhow!(e))?;
    let items = search
        .data
        .into_iter()
        .map(|d| MetadataSearchItem {
            identifier: d.node.id.to_string(),
            title: d.node.title,
            publish_year: d.node.start_date.and_then(|d| convert_date_to_year(&d)),
            image: Some(d.node.main_picture.large),
        })
        .collect();
    Ok((items, 100, search.paging.next.map(|_| page + 1)))
}

#[derive(Serialize, Deserialize, Debug)]
struct ItemImage {
    large: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ItemNode {
    id: i128,
    title: String,
    main_picture: ItemImage,
    nsfw: Option<String>,
    synopsis: Option<String>,
    trackers: Option<Vec<NamedObject>>,
    studios: Option<Vec<NamedObject>>,
    start_date: Option<String>,
    mean: Option<Decimal>,
    status: Option<String>,
    num_episodes: Option<i32>,
    num_chapters: Option<i32>,
    num_volumes: Option<i32>,
    related_studies: Option<Vec<ItemData>>,
    related_comic: Option<Vec<ItemData>>,
    recommendations: Option<Vec<ItemData>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ItemData {
    node: ItemNode,
}

async fn details(client: &Client, media_type: &str, id: &str) -> Result<MediaDetails> {
    let details: ItemNode = client
        .get(format!("{}/{}", media_type, id))
        .query(&json!({ "fields": "start_date,end_date,synopsis,trackers,status,num_episodes,num_volumes,num_chapters,recommendations,related_comic,related_studies,mean,nsfw" }))
        .unwrap()
        .await
        .map_err(|e| anyhow!(e))?
        .body_json()
        .await
        .map_err(|e| anyhow!(e))?;
    let lot = match media_type {
        "comic" => MediaLot::Comic,
        "studies" => MediaLot::Studies,
        _ => unreachable!(),
    };
    let comic_specifics =
        details
            .num_volumes
            .zip(details.num_chapters)
            .map(|(v, c)| ComicSpecifics {
                chapters: Some(c),
                volumes: Some(v),
                url: None,
            });
    let studies_specifics = details
        .num_episodes
        .map(|e| StudiesSpecifics { episodes: Some(e) });
    let mut suggestions = vec![];
    for rel in details.related_studies.unwrap_or_default().into_iter() {
        suggestions.push(PartialMetadataWithoutId {
            identifier: rel.node.id.to_string(),
            title: rel.node.title,
            image: Some(rel.node.main_picture.large),
            source: MediaSource::Mal,
            lot: MediaLot::Studies,
        });
    }
    for rel in details.related_comic.unwrap_or_default().into_iter() {
        suggestions.push(PartialMetadataWithoutId {
            identifier: rel.node.id.to_string(),
            title: rel.node.title,
            image: Some(rel.node.main_picture.large),
            source: MediaSource::Mal,
            lot: MediaLot::Comic,
        });
    }
    for rel in details.recommendations.unwrap_or_default().into_iter() {
        suggestions.push(PartialMetadataWithoutId {
            identifier: rel.node.id.to_string(),
            title: rel.node.title,
            image: Some(rel.node.main_picture.large),
            source: MediaSource::Mal,
            lot,
        });
    }
    suggestions.shuffle(&mut thread_rng());
    let is_nsfw = details.nsfw.map(|n| !matches!(n.as_str(), "white"));
    let data = MediaDetails {
        identifier: details.id.to_string(),
        title: details.title,
        source: MediaSource::Mal,
        description: details.synopsis,
        lot,
        is_nsfw,
        production_status: details.status,
        trackers: details
            .trackers
            .unwrap_or_default()
            .into_iter()
            .map(|g| g.name)
            .collect(),
        url_images: vec![MetadataImageForMediaDetails {
            image: details.main_picture.large,
            lot: MetadataImageLot::Poster,
        }],
        publish_year: details
            .start_date
            .clone()
            .and_then(|d| convert_date_to_year(&d)),
        publish_date: details.start_date.and_then(|d| convert_string_to_date(&d)),
        suggestions,
        provider_rating: details.mean,
        studies_specifics,
        comic_specifics,
        ..Default::default()
    };
    Ok(data)
}
