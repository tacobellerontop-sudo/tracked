//! Look up another player by Riot ID.

use egui::RichText;

use super::{career, widgets};
use crate::app::AppState;
use crate::ipc::{ClientStatus, Intent, IntentTx};

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.heading("Player lookup");
    ui.add_space(6.0);
    let authed = matches!(state.status, ClientStatus::Authed { .. });
    ui.horizontal(|ui| {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut state.lookup.input)
                .hint_text("Name#TAG")
                .desired_width(240.0),
        );
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let go = ui
            .add_enabled(authed && !state.lookup.loading, egui::Button::new("Search"))
            .clicked();
        if (go || enter) && authed {
            match parse_riot_id(&state.lookup.input) {
                Some((name, tag)) => {
                    state.lookup.loading = true;
                    state.lookup.result = None;
                    intents.send(Intent::LookupPlayer { name, tag });
                }
                None => state.lookup.result = Some(Err("Enter a Riot ID like Name#TAG".into())),
            }
        }
        if state.lookup.loading {
            ui.spinner();
        }
    });
    if !authed {
        ui.weak("Start VALORANT to look players up.");
        return;
    }
    ui.weak("Finds friends, recent teammates/opponents and lobby players. Click a player on the Live screen to look them up.");
    ui.add_space(10.0);

    let card = match &state.lookup.result {
        None => return,
        Some(Err(e)) => {
            ui.colored_label(widgets::LOSS, e);
            return;
        }
        Some(Ok(card)) => card.clone(),
    };
    let cat = state.catalog.clone();
    ui.horizontal(|ui| {
        widgets::rank_icon(ui, &cat, card.probe.rank.tier, 64.0);
        ui.vertical(|ui| {
            let name = card
                .riot_id
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_else(|| "Unknown player".into());
            ui.label(RichText::new(name).strong().size(18.0));
            let mut rank = cat.tier_name(card.probe.rank.tier);
            if card.probe.rank.tier > 0 {
                rank.push_str(&format!(" · {} RR", card.probe.rank.rr));
            }
            ui.label(rank);
            let mut extra = Vec::new();
            if let Some(wr) = card.probe.rank.win_rate() {
                extra.push(format!(
                    "{wr:.0}% win rate this act ({} games)",
                    card.probe.rank.season_games
                ));
            }
            if card.probe.rank.peak_tier > 0 {
                extra.push(format!("peak {}", cat.tier_name(card.probe.rank.peak_tier)));
            }
            let (w, n) = card.probe.recent_wins();
            if n > 0 {
                extra.push(format!("{w}W–{}L last {n} ranked", n - w));
            }
            ui.weak(extra.join(" · "));
        });
    });
    ui.add_space(8.0);
    ui.label(RichText::new("Recent matches").strong());
    if let Some(action) = career::match_table(ui, state, &card.puuid, &card.matches, "lookup") {
        action.apply(state, Some(card.puuid.clone()), intents);
    }
}

/// "Name#TAG" -> ("Name", "TAG"). Names may contain spaces; tags may not.
pub fn parse_riot_id(s: &str) -> Option<(String, String)> {
    let (name, tag) = s.trim().rsplit_once('#')?;
    let (name, tag) = (name.trim(), tag.trim());
    let ok = !name.is_empty() && name.chars().count() <= 16 && (3..=5).contains(&tag.chars().count());
    ok.then(|| (name.to_string(), tag.to_string()))
}

#[cfg(test)]
mod tests {
    use super::parse_riot_id;

    #[test]
    fn riot_ids() {
        assert_eq!(parse_riot_id("Some Name#EUW"), Some(("Some Name".into(), "EUW".into())));
        assert_eq!(parse_riot_id(" a#b#1234 "), Some(("a#b".into(), "1234".into())));
        assert_eq!(parse_riot_id("noTag"), None);
        assert_eq!(parse_riot_id("#123"), None);
        assert_eq!(parse_riot_id("x#toolong"), None);
    }
}
