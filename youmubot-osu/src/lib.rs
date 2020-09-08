pub mod discord;
pub mod models;
pub mod request;

#[cfg(test)]
mod test;

use models::*;
use request::builders::*;
use request::*;
use reqwest::{Client as HTTPClient, RequestBuilder, Response};
use std::{convert::TryInto, sync::Arc};
use tower;
use youmubot_prelude::*;

/// The number of requests per minute to the osu! server.
const REQUESTS_PER_MINUTE: u64 = 200;

type BoxedResp =
    std::pin::Pin<Box<dyn future::Future<Output = Result<Response, reqwest::Error>> + Send>>;
/// Client is the client that will perform calls to the osu! api server.
pub struct Client {
    http_client: RwLock<
        Box<
            dyn tower::Service<
                    reqwest::Request,
                    Response = Response,
                    Error = reqwest::Error,
                    Future = BoxedResp,
                > + Send
                + Sync,
        >,
    >,
    client: Arc<HTTPClient>,
    key: String,
}

fn vec_try_into<U, T: std::convert::TryFrom<U>>(v: Vec<U>) -> Result<Vec<T>, T::Error> {
    let mut res = Vec::with_capacity(v.len());

    for u in v.into_iter() {
        res.push(u.try_into()?);
    }

    Ok(res)
}

impl Client {
    /// Create a new client from the given API key.
    pub fn new(key: String) -> Client {
        let http_client = Arc::new(HTTPClient::new());
        let _http = http_client.clone();
        let srv = tower::ServiceBuilder::new()
            .rate_limit(REQUESTS_PER_MINUTE, std::time::Duration::from_secs(60))
            .service(tower::service_fn(move |req| -> BoxedResp {
                Box::pin(_http.execute(req))
            }));
        Client {
            key,
            http_client: RwLock::new(Box::new(srv)),
            client: http_client,
        }
    }

    async fn build_request(&self, r: RequestBuilder) -> Result<Response> {
        let v = r.query(&[("k", &*self.key)]).build()?;
        let mut client = self.http_client.write().await;
        future::poll_fn(|ctx| client.poll_ready(ctx)).await?;
        Ok(client.call(v).await?)
    }

    pub async fn beatmaps(
        &self,
        kind: BeatmapRequestKind,
        f: impl FnOnce(&mut BeatmapRequestBuilder) -> &mut BeatmapRequestBuilder,
    ) -> Result<Vec<Beatmap>> {
        let mut r = BeatmapRequestBuilder::new(kind);
        f(&mut r);
        let res: Vec<raw::Beatmap> = self
            .build_request(r.build(&self.client))
            .await?
            .json()
            .await?;
        Ok(vec_try_into(res)?)
    }

    pub async fn user(
        &self,
        user: UserID,
        f: impl FnOnce(&mut UserRequestBuilder) -> &mut UserRequestBuilder,
    ) -> Result<Option<User>, Error> {
        let mut r = UserRequestBuilder::new(user);
        f(&mut r);
        let res: Vec<raw::User> = self
            .build_request(r.build(&self.client))
            .await?
            .json()
            .await?;
        let res = vec_try_into(res)?;
        Ok(res.into_iter().next())
    }

    pub async fn scores(
        &self,
        beatmap_id: u64,
        f: impl FnOnce(&mut ScoreRequestBuilder) -> &mut ScoreRequestBuilder,
    ) -> Result<Vec<Score>, Error> {
        let mut r = ScoreRequestBuilder::new(beatmap_id);
        f(&mut r);
        let res: Vec<raw::Score> = self
            .build_request(r.build(&self.client))
            .await?
            .json()
            .await?;
        let mut res: Vec<Score> = vec_try_into(res)?;

        // with a scores request you need to fill the beatmap ids yourself
        res.iter_mut().for_each(|v| {
            v.beatmap_id = beatmap_id;
        });
        Ok(res)
    }

    pub async fn user_best(
        &self,
        user: UserID,
        f: impl FnOnce(&mut UserScoreRequestBuilder) -> &mut UserScoreRequestBuilder,
    ) -> Result<Vec<Score>, Error> {
        self.user_scores(UserScoreType::Best, user, f).await
    }

    pub async fn user_recent(
        &self,
        user: UserID,
        f: impl FnOnce(&mut UserScoreRequestBuilder) -> &mut UserScoreRequestBuilder,
    ) -> Result<Vec<Score>, Error> {
        self.user_scores(UserScoreType::Recent, user, f).await
    }

    async fn user_scores(
        &self,
        u: UserScoreType,
        user: UserID,
        f: impl FnOnce(&mut UserScoreRequestBuilder) -> &mut UserScoreRequestBuilder,
    ) -> Result<Vec<Score>, Error> {
        let mut r = UserScoreRequestBuilder::new(u, user);
        f(&mut r);
        let res: Vec<raw::Score> = self
            .build_request(r.build(&self.client))
            .await?
            .json()
            .await?;
        let res = vec_try_into(res)?;
        Ok(res)
    }
}
