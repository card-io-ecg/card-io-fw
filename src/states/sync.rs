use crate::{
    board::initialized::{Context, StaMode},
    states::{
        firmware_update::firmware_update, menu::AppMenu, upload_or_store_measurement::upload_stored,
    },
    AppState,
};

/// Uploads the stored measurements and the log records, then looks for a firmware update.
pub async fn sync(context: &mut Context) -> AppState {
    if !upload_stored(context, StaMode::Enable).await {
        return AppState::Menu(AppMenu::Main);
    }

    firmware_update(context).await
}
