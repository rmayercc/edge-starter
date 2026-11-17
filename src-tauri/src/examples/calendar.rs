//! Example: today's and tomorrow's meetings from the macOS Calendar
//! (EventKit), each with a Join link. Whatever syncs into Calendar shows up
//! here (iCloud, Google, Microsoft 365 / Exchange…), so there's no separate
//! sign-in. macOS asks once for Calendar access (`calendar_request_access`).
//! Needs the Calendars keys in Info.plist and Entitlements.plist.
//!
//! The join link comes from the event's URL, location or notes: Teams, Zoom,
//! Google Meet or Webex. Declined and cancelled events are skipped.
//!
//! `start()` also watches EventKit and sends the page an `eventkit-changed`
//! event when calendars or reminders change.

use super::reminders::Access;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

/// The event the page listens for to refresh the calendar and to-do cards.
pub const CHANGED: &str = "eventkit-changed";

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub title: String,
    /// Unix seconds.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    /// The calendar it's in ("Calendar", "Work"…).
    pub calendar: String,
    pub location: Option<String>,
    /// "Teams" | "Zoom" | "Meet" | "Webex" if there's a link to join.
    pub join: Option<&'static str>,
}

/// Join links by event id, from the last `list()`. The page only says which
/// event to join; the link never comes from the page.
static LINKS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Which meeting service a host belongs to.
pub fn provider(host: &str) -> Option<&'static str> {
    let host = host.to_lowercase();
    let is = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if is("teams.microsoft.com") || is("teams.live.com") || is("teams.microsoft.us") {
        Some("Teams")
    } else if is("zoom.us") || is("zoomgov.com") {
        Some("Zoom")
    } else if is("meet.google.com") {
        Some("Meet")
    } else if is("webex.com") {
        Some("Webex")
    } else {
        None
    }
}

/// Every http(s) URL in a piece of text (Outlook wraps them in <…>).
fn urls(text: &str) -> impl Iterator<Item = url::Url> + '_ {
    let mut rest = text;
    std::iter::from_fn(move || loop {
        let at = rest.find("http")?;
        let tail = &rest[at..];
        let end = tail.find(|c: char| c.is_whitespace() || "<>\"'()[]{}".contains(c)).unwrap_or(tail.len());
        let candidate = tail[..end].trim_end_matches(['.', ',', ';', '!', '?']);
        rest = &tail[end.max(4)..];
        if candidate.starts_with("https://") || candidate.starts_with("http://") {
            if let Ok(u) = url::Url::parse(candidate) {
                return Some(u);
            }
        }
    })
}

/// The first meeting link in the given texts, in order: (link, provider).
/// Looks through Outlook "safe links" wrappers too.
pub fn find_join_link(texts: &[&str]) -> Option<(String, &'static str)> {
    texts.iter().flat_map(|t| urls(t)).find_map(|u| {
        let host = u.host_str()?.to_string();
        if let Some(p) = provider(&host) {
            return Some((u.to_string(), p));
        }
        if host.to_lowercase().ends_with(".safelinks.protection.outlook.com") {
            let inner = u.query_pairs().find(|(k, _)| k == "url")?.1.into_owned();
            let inner = url::Url::parse(&inner).ok()?;
            let p = provider(inner.host_str()?)?;
            return Some((inner.to_string(), p));
        }
        None
    })
}

/// What to hand to `open`: a Teams meeting link goes straight to the Teams
/// app (when `teams_installed`) instead of a browser page asking to open it.
pub fn launch_target(link: &str, teams_installed: bool) -> String {
    if teams_installed {
        if let Some(rest) = link.strip_prefix("https://teams.microsoft.com/") {
            if rest.starts_with("l/meetup-join/") || rest.starts_with("meet/") {
                return format!("msteams://teams.microsoft.com/{rest}");
            }
        }
    }
    link.to_string()
}

pub fn sort(events: &mut [Event]) {
    events.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())));
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEvent, EKEventStatus, EKEventStore, EKEventStoreChangedNotification, EKParticipantStatus};
    use objc2_foundation::{NSDate, NSError};
    use std::sync::mpsc;
    use std::time::Duration;

    pub fn access() -> Access {
        let s = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        if s == EKAuthorizationStatus::FullAccess {
            Access::Granted
        } else if s == EKAuthorizationStatus::NotDetermined {
            Access::Unknown
        } else {
            Access::Denied
        }
    }

    pub fn request_access() -> Access {
        let store = unsafe { EKEventStore::new() };
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |_granted: Bool, _err: *mut NSError| {
            let _ = tx.send(());
        });
        unsafe { store.requestFullAccessToEventsWithCompletion(&*block as *const _ as *mut _) };
        let _ = rx.recv_timeout(Duration::from_secs(120));
        let a = access();
        crate::log(&format!("calendar: access after prompt: {a:?}"));
        // A store made before access was granted doesn't see the calendars.
        watch();
        a
    }

    /// (event, join link), or None for cancelled and declined events.
    unsafe fn to_event(e: &EKEvent) -> Option<(Event, Option<String>)> {
        if e.status() == EKEventStatus::Canceled {
            return None;
        }
        if e.attendees().is_some_and(|all| all.iter().any(|p| p.isCurrentUser() && p.participantStatus() == EKParticipantStatus::Declined)) {
            return None;
        }
        let start = e.startDate().timeIntervalSince1970() as i64;
        let text = |s: Option<Retained<objc2_foundation::NSString>>| s.map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
        let url = text(e.URL().and_then(|u| u.absoluteString()));
        let location = text(e.location());
        let notes = text(e.notes());
        let link = find_join_link(&[url.as_deref().unwrap_or(""), location.as_deref().unwrap_or(""), notes.as_deref().unwrap_or("")]);
        let event = Event {
            // Every occurrence of a repeating event shares an identifier.
            id: format!("{}@{start}", e.eventIdentifier().map(|s| s.to_string()).unwrap_or_default()),
            title: text(Some(e.title())).unwrap_or_else(|| "(No title)".into()),
            start,
            end: e.endDate().timeIntervalSince1970() as i64,
            all_day: e.isAllDay(),
            calendar: e.calendar().map(|c| c.title().to_string()).unwrap_or_default(),
            // A location that is just the meeting link isn't worth showing.
            location: location.filter(|l| !l.contains("://")),
            join: link.as_ref().map(|(_, p)| *p),
        };
        Some((event, link.map(|(l, _)| l)))
    }

    /// Everything from the start of today to the end of tomorrow.
    pub fn list() -> Vec<Event> {
        if access() != Access::Granted {
            return vec![];
        }
        let today = chrono::Local::now().date_naive();
        let at = |d: chrono::NaiveDate| {
            let t = chrono::TimeZone::from_local_datetime(&chrono::Local, &d.and_hms_opt(0, 0, 0).unwrap()).earliest().map(|t| t.timestamp()).unwrap_or(0);
            NSDate::dateWithTimeIntervalSince1970(t as f64)
        };
        let (from, to) = (at(today), at(today + chrono::Days::new(2)));
        let mut links = HashMap::new();
        let mut events: Vec<Event> = unsafe {
            let store = EKEventStore::new();
            let pred = store.predicateForEventsWithStartDate_endDate_calendars(&from, &to, None);
            store
                .eventsMatchingPredicate(&pred)
                .iter()
                .filter_map(|e| to_event(&e))
                .map(|(event, link)| {
                    if let Some(link) = link {
                        links.insert(event.id.clone(), link);
                    }
                    event
                })
                .collect()
        };
        *LINKS.lock().unwrap() = Some(links);
        sort(&mut events);
        events
    }

    struct Watch(#[allow(dead_code)] Retained<EKEventStore>, #[allow(dead_code)] Retained<AnyObject>);
    // Only held to keep the store and its observer alive.
    unsafe impl Send for Watch {}
    static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

    /// Tell the page whenever EventKit says calendars or reminders changed.
    pub fn watch() {
        unsafe {
            let store = EKEventStore::new();
            let block = RcBlock::new(|_note: *mut AnyObject| crate::emit(CHANGED));
            let center: Retained<AnyObject> = msg_send![class!(NSNotificationCenter), defaultCenter];
            let mut held = WATCH.lock().unwrap();
            if let Some(Watch(_, old)) = held.take() {
                let _: () = msg_send![&*center, removeObserver: &*old];
            }
            let token: Retained<AnyObject> = msg_send![&*center, addObserverForName: EKEventStoreChangedNotification, object: &*store, queue: std::ptr::null::<AnyObject>(), usingBlock: &*block];
            *held = Some(Watch(store, token));
        }
    }
}

pub fn access() -> Access {
    #[cfg(target_os = "macos")]
    return mac::access();
    #[cfg(not(target_os = "macos"))]
    Access::Denied
}

pub fn request_access() -> Access {
    #[cfg(target_os = "macos")]
    return mac::request_access();
    #[cfg(not(target_os = "macos"))]
    Access::Denied
}

pub fn list() -> Vec<Event> {
    #[cfg(target_os = "macos")]
    return mac::list();
    #[cfg(not(target_os = "macos"))]
    vec![]
}

/// Watch EventKit for changes. Call once at startup.
pub fn start() {
    #[cfg(target_os = "macos")]
    mac::watch();
}

/// Open the meeting for an event from the last `list()`.
pub fn join(id: &str) -> Result<(), String> {
    let link = LINKS.lock().unwrap().as_ref().and_then(|l| l.get(id).cloned()).ok_or("That meeting has no link to join")?;
    let teams = ["/Applications/Microsoft Teams.app", "/Applications/Microsoft Teams (work or school).app"].iter().any(|p| std::path::Path::new(p).exists());
    let target = launch_target(&link, teams);
    let status = std::process::Command::new("/usr/bin/open").arg(&target).status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        crate::log(&format!("calendar: open failed for a {} link", provider(url::Url::parse(&link).ok().and_then(|u| u.host_str().map(str::to_string)).as_deref().unwrap_or("")).unwrap_or("meeting")));
        Err("Couldn't open the meeting".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_meeting_links() {
        let teams = "Join on your computer <https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0?context=%7b%7d> or call in";
        assert_eq!(find_join_link(&["", "Microsoft Teams Meeting", teams]).unwrap().1, "Teams");
        assert_eq!(find_join_link(&["https://acme.zoom.us/j/123456789?pwd=abc."]).unwrap(), ("https://acme.zoom.us/j/123456789?pwd=abc".to_string(), "Zoom"));
        assert_eq!(find_join_link(&["Call: (https://meet.google.com/abc-defg-hij)"]).unwrap().0, "https://meet.google.com/abc-defg-hij");
        assert_eq!(find_join_link(&["https://acme.webex.com/meet/someone"]).unwrap().1, "Webex");
        // The event's own URL wins over one in the notes.
        assert_eq!(find_join_link(&["https://zoom.us/j/1", "", "https://meet.google.com/x"]).unwrap().1, "Zoom");
    }

    #[test]
    fn ignores_other_links() {
        assert_eq!(find_join_link(&["Agenda: https://example.com/doc and http://notzoom.us/j/1", "Room 4"]), None);
        assert_eq!(find_join_link(&["https://evil.example/?next=https://zoom.us/j/1"]), None);
        assert_eq!(find_join_link(&["no links here, http and https mentioned"]), None);
    }

    #[test]
    fn unwraps_outlook_safe_links() {
        let safe = "https://nam02.safelinks.protection.outlook.com/?url=https%3A%2F%2Fteams.microsoft.com%2Fl%2Fmeetup-join%2Fabc&data=05%7C01";
        assert_eq!(find_join_link(&[safe]).unwrap(), ("https://teams.microsoft.com/l/meetup-join/abc".to_string(), "Teams"));
        assert_eq!(find_join_link(&["https://nam02.safelinks.protection.outlook.com/?url=https%3A%2F%2Fexample.com"]), None);
    }

    #[test]
    fn teams_links_open_in_the_app() {
        assert_eq!(launch_target("https://teams.microsoft.com/l/meetup-join/abc", true), "msteams://teams.microsoft.com/l/meetup-join/abc");
        assert_eq!(launch_target("https://teams.microsoft.com/l/meetup-join/abc", false), "https://teams.microsoft.com/l/meetup-join/abc");
        assert_eq!(launch_target("https://zoom.us/j/1", true), "https://zoom.us/j/1");
    }
}
