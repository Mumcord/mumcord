//! The sender's request queue (FR-7.17, FR-7.18): one transfer at a time across all shares on all
//! servers, everyone else waits in FIFO order. Pure state; the caller sends the returned
//! `mumcord.file` messages and starts or aborts the transfers.

use std::collections::VecDeque;

use mumcord_store::ServerId;

use super::wire::FileMessage;

/// Most requests that may wait per sender; one more gets `busy` (FR-7.17).
pub const MAX_WAITING: usize = 50;

/// A recipient: a user session on one server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Requester {
    pub server_id: ServerId,
    pub session: u32,
}

/// A recipient's request for one share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub share_id: String,
    pub requester: Requester,
    /// The file stream format chosen for this recipient (see [`super::transfer`]).
    pub format: u8,
}

/// A message for one recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub to: Requester,
    pub message: FileMessage,
}

/// What the caller has to do after a queue change.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Messages to send, in order.
    pub send: Vec<Outgoing>,
    /// The running transfer to stop.
    pub abort: Option<Request>,
    /// The transfer to start now (its `go` is already in `send`).
    pub start: Option<Request>,
}

/// Global sender queue.
#[derive(Debug, Default)]
pub struct SenderQueue {
    running: Option<Request>,
    waiting: VecDeque<Request>,
}

impl SenderQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn running(&self) -> Option<&Request> {
        self.running.as_ref()
    }

    /// Waiting requests, first in line first.
    pub fn waiting(&self) -> impl Iterator<Item = &Request> {
        self.waiting.iter()
    }

    /// Number of requests waiting for `share_id` (the card's "{n} waiting").
    pub fn waiting_for(&self, share_id: &str) -> usize {
        self.waiting
            .iter()
            .filter(|r| r.share_id == share_id)
            .count()
    }

    /// A `req` arrived; `format` is the file stream format chosen for it. `blocked`: the sender
    /// blocked the requester; `active`: the share is in the registry. Duplicates of a queued or
    /// running request are ignored.
    pub fn request(
        &mut self,
        share_id: &str,
        requester: Requester,
        format: u8,
        blocked: bool,
        active: bool,
    ) -> Step {
        let id = share_id.to_owned();
        if blocked || !active {
            return Step {
                send: vec![Outgoing {
                    to: requester,
                    message: FileMessage::Gone { id },
                }],
                ..Step::default()
            };
        }
        let request = Request {
            share_id: id,
            requester,
            format,
        };
        if self.running.as_ref() == Some(&request) || self.waiting.contains(&request) {
            return Step::default();
        }
        if self.running.is_none() {
            return self.start(request);
        }
        if self.waiting.len() >= MAX_WAITING {
            return Step {
                send: vec![Outgoing {
                    to: requester,
                    message: FileMessage::Busy {
                        id: request.share_id,
                    },
                }],
                ..Step::default()
            };
        }
        self.waiting.push_back(request);
        let position = self.waiting.len();
        Step {
            send: vec![queued(&self.waiting[position - 1], position)],
            ..Step::default()
        }
    }

    /// The running transfer finished, failed or was cancelled by the sender: next in line goes.
    pub fn finish(&mut self) -> Step {
        self.running = None;
        self.advance(Step::default(), 0)
    }

    /// The recipient sent `cancel` for `share_id`.
    pub fn cancel(&mut self, share_id: &str, requester: Requester) -> Step {
        self.remove(
            |r| r.share_id == share_id && r.requester == requester,
            false,
        )
    }

    /// The recipient left the server: drop all their requests.
    pub fn drop_requester(&mut self, requester: Requester) -> Step {
        self.remove(|r| r.requester == requester, false)
    }

    /// The share ended: waiting recipients get `gone`, a running transfer of it is aborted.
    pub fn end_share(&mut self, share_id: &str) -> Step {
        self.remove(|r| r.share_id == share_id, true)
    }

    /// The sender left `server_id`: drop its requests silently, there is no connection to answer
    /// on. Requests on other servers move up.
    pub fn end_server(&mut self, server_id: ServerId) -> Step {
        let mut step = self.remove(|r| r.requester.server_id == server_id, false);
        step.send.retain(|o| o.to.server_id != server_id);
        step
    }

    fn remove(&mut self, matches: impl Fn(&Request) -> bool, gone: bool) -> Step {
        let mut step = Step::default();
        let notify = |request: &Request, step: &mut Step| {
            if gone {
                step.send.push(Outgoing {
                    to: request.requester,
                    message: FileMessage::Gone {
                        id: request.share_id.clone(),
                    },
                });
            }
        };
        if let Some(running) = self.running.take_if(|r| matches(r)) {
            notify(&running, &mut step);
            step.abort = Some(running);
        }
        let first_changed = self.waiting.iter().position(&matches);
        let mut kept = VecDeque::with_capacity(self.waiting.len());
        for request in self.waiting.drain(..) {
            if matches(&request) {
                notify(&request, &mut step);
            } else {
                kept.push_back(request);
            }
        }
        self.waiting = kept;
        match (self.running.is_none(), first_changed) {
            (true, _) => self.advance(step, 0),
            (false, Some(from)) => {
                self.announce_positions(&mut step, from);
                step
            }
            (false, None) => step,
        }
    }

    /// Starts the next waiting request if nothing runs, then tells everyone from index `from`
    /// on their (new) position.
    fn advance(&mut self, mut step: Step, from: usize) -> Step {
        if self.running.is_none()
            && let Some(next) = self.waiting.pop_front()
        {
            let started = self.start(next);
            step.send.extend(started.send);
            step.start = started.start;
        }
        self.announce_positions(&mut step, from);
        step
    }

    fn announce_positions(&self, step: &mut Step, from: usize) {
        for (i, request) in self.waiting.iter().enumerate().skip(from) {
            step.send.push(queued(request, i + 1));
        }
    }

    fn start(&mut self, request: Request) -> Step {
        self.running = Some(request.clone());
        Step {
            send: vec![Outgoing {
                to: request.requester,
                message: FileMessage::Go {
                    id: request.share_id.clone(),
                    fmt: request.format,
                },
            }],
            start: Some(request),
            abort: None,
        }
    }
}

fn queued(request: &Request, position: usize) -> Outgoing {
    Outgoing {
        to: request.requester,
        message: FileMessage::Queued {
            id: request.share_id.clone(),
            // MAX_WAITING keeps positions far below u16::MAX.
            position: u16::try_from(position).unwrap_or(u16::MAX),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(server_id: ServerId, session: u32) -> Requester {
        Requester { server_id, session }
    }

    fn go(to: Requester, id: &str) -> Outgoing {
        Outgoing {
            to,
            message: FileMessage::Go {
                id: id.into(),
                fmt: 1,
            },
        }
    }

    fn pos(to: Requester, id: &str, position: u16) -> Outgoing {
        Outgoing {
            to,
            message: FileMessage::Queued {
                id: id.into(),
                position,
            },
        }
    }

    fn gone(to: Requester, id: &str) -> Outgoing {
        Outgoing {
            to,
            message: FileMessage::Gone { id: id.into() },
        }
    }

    fn req(id: &str, requester: Requester) -> Request {
        Request {
            share_id: id.into(),
            requester,
            format: 1,
        }
    }

    #[test]
    fn first_request_goes_others_queue_in_order() {
        let mut q = SenderQueue::new();
        let (a, b, c) = (who(1, 10), who(2, 20), who(1, 30));
        let step = q.request("s1", a, 1, false, true);
        assert_eq!(step.send, [go(a, "s1")]);
        assert_eq!(step.start, Some(req("s1", a)));
        assert_eq!(q.request("s2", b, 1, false, true).send, [pos(b, "s2", 1)]);
        assert_eq!(q.request("s1", c, 1, false, true).send, [pos(c, "s1", 2)]);
        assert_eq!(q.waiting_for("s1"), 1);

        let step = q.finish();
        assert_eq!(step.send, [go(b, "s2"), pos(c, "s1", 1)]);
        assert_eq!(step.start, Some(req("s2", b)));
        assert_eq!(q.finish().send, [go(c, "s1")]);
        let step = q.finish();
        assert_eq!(step, Step::default());
        assert!(q.running().is_none());
    }

    #[test]
    fn blocked_or_inactive_get_gone() {
        let mut q = SenderQueue::new();
        let a = who(1, 1);
        assert_eq!(q.request("s", a, 1, true, true).send, [gone(a, "s")]);
        assert_eq!(q.request("s", a, 1, false, false).send, [gone(a, "s")]);
        assert!(q.running().is_none());
    }

    #[test]
    fn duplicates_are_ignored() {
        let mut q = SenderQueue::new();
        let (a, b) = (who(1, 1), who(1, 2));
        q.request("s", a, 1, false, true);
        assert_eq!(q.request("s", a, 1, false, true), Step::default());
        q.request("s", b, 1, false, true);
        assert_eq!(q.request("s", b, 1, false, true), Step::default());
        // Same user, other share is a new request.
        assert_eq!(q.request("t", b, 1, false, true).send, [pos(b, "t", 2)]);
    }

    #[test]
    fn fifty_wait_the_next_is_busy() {
        let mut q = SenderQueue::new();
        q.request("s", who(1, 0), 1, false, true);
        for session in 1..=MAX_WAITING as u32 {
            let step = q.request("s", who(1, session), 1, false, true);
            assert_eq!(step.send, [pos(who(1, session), "s", session as u16)]);
        }
        let late = who(1, 999);
        assert_eq!(
            q.request("s", late, 1, false, true).send,
            [Outgoing {
                to: late,
                message: FileMessage::Busy { id: "s".into() }
            }]
        );
        assert_eq!(q.waiting().count(), MAX_WAITING);
    }

    #[test]
    fn cancel_queued_moves_later_ones_up() {
        let mut q = SenderQueue::new();
        let (a, b, c, d) = (who(1, 1), who(1, 2), who(1, 3), who(1, 4));
        for r in [a, b, c, d] {
            q.request("s", r, 1, false, true);
        }
        let step = q.cancel("s", c);
        assert_eq!(step.send, [pos(d, "s", 2)]);
        assert_eq!(step.abort, None);
        assert_eq!(q.cancel("s", c), Step::default());
    }

    #[test]
    fn cancel_running_aborts_and_starts_next() {
        let mut q = SenderQueue::new();
        let (a, b, c) = (who(1, 1), who(1, 2), who(1, 3));
        for r in [a, b, c] {
            q.request("s", r, 1, false, true);
        }
        let step = q.cancel("s", a);
        assert_eq!(step.abort, Some(req("s", a)));
        assert_eq!(step.start, Some(req("s", b)));
        assert_eq!(step.send, [go(b, "s"), pos(c, "s", 1)]);
    }

    #[test]
    fn requester_leaving_drops_all_their_requests() {
        let mut q = SenderQueue::new();
        let (a, b) = (who(1, 1), who(1, 2));
        q.request("s", a, 1, false, true);
        q.request("s", b, 1, false, true);
        q.request("t", a, 1, false, true);
        let step = q.drop_requester(a);
        assert_eq!(step.abort, Some(req("s", a)));
        assert_eq!(step.send, [go(b, "s")]);
        assert_eq!(q.waiting().count(), 0);
    }

    #[test]
    fn share_end_sends_gone_and_aborts() {
        let mut q = SenderQueue::new();
        let (a, b, c) = (who(1, 1), who(1, 2), who(2, 3));
        q.request("s", a, 1, false, true);
        q.request("s", b, 1, false, true);
        q.request("t", c, 1, false, true);
        let step = q.end_share("s");
        assert_eq!(step.abort, Some(req("s", a)));
        assert_eq!(step.send, [gone(a, "s"), gone(b, "s"), go(c, "t")]);
        assert_eq!(step.start, Some(req("t", c)));
    }

    #[test]
    fn share_end_while_other_runs_updates_positions() {
        let mut q = SenderQueue::new();
        let (a, b, c) = (who(1, 1), who(1, 2), who(1, 3));
        q.request("t", a, 1, false, true);
        q.request("s", b, 1, false, true);
        q.request("t", c, 1, false, true);
        let step = q.end_share("s");
        assert_eq!(step.abort, None);
        assert_eq!(step.send, [gone(b, "s"), pos(c, "t", 1)]);
    }

    #[test]
    fn sender_leaving_a_server_is_silent_there() {
        let mut q = SenderQueue::new();
        let (a, b, c) = (who(1, 1), who(2, 2), who(1, 3));
        q.request("s", a, 1, false, true);
        q.request("t", b, 1, false, true);
        q.request("s", c, 1, false, true);
        let step = q.end_server(1);
        assert_eq!(step.abort, Some(req("s", a)));
        assert_eq!(step.send, [go(b, "t")]);
        assert_eq!(q.waiting().count(), 0);
    }

    #[test]
    fn go_names_the_stream_format_of_its_request() {
        let mut q = SenderQueue::new();
        let (a, b) = (who(1, 10), who(1, 20));
        let step = q.request("s", a, 2, false, true);
        assert_eq!(
            step.send[0].message,
            FileMessage::Go {
                id: "s".into(),
                fmt: 2
            }
        );
        q.request("s", b, 1, false, true);
        let step = q.cancel("s", a);
        let go = step.send.iter().find(|o| o.to == b).expect("b starts");
        assert_eq!(
            go.message,
            FileMessage::Go {
                id: "s".into(),
                fmt: 1
            }
        );
    }
}
