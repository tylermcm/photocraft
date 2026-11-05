//! Desktop service differences owned by the Image Triage host.
use photocraft_ui_egui::Services;

pub(crate) fn services(mut services: Services, hosted: bool) -> Services {
    if hosted {
        // The host distributes and validates its editor build. A standalone release
        // prompt would cover the embedded photo and offer an incompatible replacement.
        services.check_updates = None;
    }
    services
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_engine::Session;
    use photocraft_ui_egui::PhotocraftApp;

    fn with_updates() -> Services {
        Services { check_updates: Some(Box::new(|| Err("test callback must not run".into()))), ..Default::default() }
    }

    #[test]
    fn hosted_startup_has_no_standalone_update_service_or_preference_change() {
        let app = PhotocraftApp::new(Session::new(), services(with_updates(), true));
        assert!(app.session.updates.check.is_none());
        assert!(app.session.prefs().general.automatically_check_updates);
        assert!(!app.ui.updates_open);
    }

    #[test]
    fn standalone_startup_keeps_upstream_update_service() {
        let app = PhotocraftApp::new(Session::new(), services(with_updates(), false));
        assert!(app.session.updates.check.is_some());
        assert!(app.session.prefs().general.automatically_check_updates);
    }
}
