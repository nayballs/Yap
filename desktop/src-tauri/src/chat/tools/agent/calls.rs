//! A step's tool calls, one by one: their arguments fixed, gated, run and
//! folded back into the turn.

use serde_json::Value;

use super::args::call_arguments;
use super::dispatch::Source;
use super::gate::gate;
use super::Loop;

impl Loop<'_> {
    /// **One step's tool calls** (`calls`, from `msg`; `forced`: the step
    /// made to search), in the order the model made them: the assistant's
    /// message kept, then for each call its arguments fixed
    /// ([`Loop::fix_arguments`]), the guards ([`gate`]), the call run where
    /// it belongs ([`Source::of`], [`Loop::dispatch`]) and its result
    /// folded back ([`Loop::fold`]); then what follows the step
    /// ([`Loop::after_calls`]).
    pub(super) async fn run_calls(&mut self, mut msg: Value, calls: &[Value], forced: bool) {
        // Append the assistant turn (with its tool_calls), execute each call,
        // and feed the results back. A forced step's words go without it:
        // nobody saw them, and "I don't have access to real-time weather
        // data" would steer the answer from the search.
        if forced {
            msg["content"] = Value::Null;
        }
        self.messages.push(msg);
        // This step only searched the notes, and found nothing.
        let mut notes_found_nothing = true;
        // **A new document's outline before any search** (`web::Turn::
        // outline_first`): this step's searches and page reads are refused,
        // unless the step makes the outline too.
        let hold_web = self.web.as_deref().is_some_and(crate::web::Turn::outline_pending) && !calls.iter().any(|c| c["function"]["name"] == "create_document");
        // The pictures this step looked at (`look_at_picture`): one user
        // message after its tool results carries them (chat/pictures.rs).
        let mut looks: Vec<crate::chat::pictures::Look> = Vec::new();
        for call in calls {
            let id = call["id"].as_str().unwrap_or("");
            let name = call["function"]["name"].as_str().unwrap_or("");
            let mut args = call_arguments(call);
            self.fix_arguments(id, name, &mut args, forced);
            let ran_with = args.clone();
            // The page's line: what this call does, with what ("Searching the
            // web · …" — a provider's native search or Exa's alike —,
            // "Reading …", "Writing ‘Gear’").
            if let Some(live) = self.live {
                crate::chat::activity::tell(live.sink, crate::chat::activity::of_call(name, &args));
            }
            let web_seen = self.web.as_deref().is_some_and(crate::web::Turn::seen) || self.screen_seen || self.job_seen;
            // The safety rule's one exception: the person's plain go-ahead
            // to a job (`message_job` then sends Yap's own words, never the
            // model's: `tools/jobs.rs`).
            let goes_ahead = web_seen && crate::chat::tools::jobs::goes_ahead_after_web(name, self.question);
            let result = match gate(name, &args, hold_web, web_seen && !goes_ahead, &self.extra) {
                Some(refusal) => refusal,
                None => {
                    let source = Source::of(name, self.web.is_some(), &self.extra, self.claude_web, self.open_apps);
                    self.dispatch(source, name, args, &mut looks).await
                }
            };
            notes_found_nothing &= self.fold(id, name, &ran_with, result, &looks);
        }
        self.after_calls(&looks, notes_found_nothing).await;
    }
}
