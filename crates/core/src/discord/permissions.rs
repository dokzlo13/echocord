//! Bounded REST permission checks shared by independently owned Discord rooms.
use crate::{model::ErrorCode, protocol::Failure};
use std::time::Duration;
#[cfg(windows)]
pub(crate) async fn permissions(
    http: &serenity::http::Http,
    guild: serenity::all::GuildId,
    channel: serenity::all::ChannelId,
) -> Result<(), Failure> {
    use serenity::all::{Channel, ChannelType, Permissions};
    let work = async {
        let user = http.get_current_user().await.map_err(classify_rest)?;
        let guild_data = guild.to_partial_guild(http).await.map_err(classify_rest)?;
        let member = guild_data
            .member(http, user.id)
            .await
            .map_err(classify_rest)?;
        let Channel::Guild(channel_data) = channel.to_channel(http).await.map_err(classify_rest)?
        else {
            return Err(Failure::Permission);
        };
        if channel_data.guild_id != guild || channel_data.kind != ChannelType::Voice {
            return Err(Failure::Permission);
        }
        let permissions = guild_data.user_permissions_in(&channel_data, &member);
        if !permissions
            .contains(Permissions::VIEW_CHANNEL | Permissions::CONNECT | Permissions::SPEAK)
        {
            return Err(Failure::Permission);
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .unwrap_or(Err(Failure::Connection))
}
#[cfg(windows)]
pub(crate) fn classify_rest(e: serenity::Error) -> Failure {
    if matches!(&e, serenity::Error::Http(e) if e.status_code().is_some_and(|s| s.as_u16() == 404))
    {
        return Failure::Permission;
    }
    match crate::controller::rest_error(e).code {
        ErrorCode::Authentication => Failure::Authentication,
        ErrorCode::PermissionDenied => Failure::Permission,
        _ => Failure::Connection,
    }
}
