//! The execution port, the effects that the event loop hands to it and the
//! receipts that it gives back.

use std::fmt;

/// The execution port: where effects go to submit and cancel jobs, and
/// where job reports come from.
///
/// The framework defines the port and never holds one; the application's
/// assembly implements it on its job layer, and whoever drives the rounds
/// calls it. Every method returns at once and never blocks.
pub trait JobPort {
    /// A job request, which the application builds.
    type Request: fmt::Debug;
    /// The identity that an accepted submission gets; a cancellation names
    /// a job by it.
    type Submission: Clone + PartialEq + fmt::Debug;
    /// Why the port refused a submission.
    type Rejection: fmt::Debug;
    /// The port's answer to a cancellation, which only the event handling
    /// record keeps; the terminal state of the job comes as a report.
    type CancelAnswer: fmt::Debug;
    /// A job report, such as a terminal state with its result.
    type Report: fmt::Debug;

    /// Submits a job and answers at once: its identity, or why it was
    /// refused.
    fn submit(&mut self, request: Self::Request) -> Result<Self::Submission, Self::Rejection>;

    /// Cancels the job of `submission`.
    fn cancel(&mut self, submission: &Self::Submission) -> Self::CancelAnswer;

    /// Appends the reports that have arrived to `out` and returns the
    /// number of jobs accepted through the port whose terminal state has
    /// not been taken yet.
    fn take_reports(&mut self, out: &mut Vec<Self::Report>) -> usize;
}

/// An effect: a value that `update` and the event handlers of controls
/// output, which the event loop hands to the execution port in the order
/// they were produced. A new kind of effect is a new variant.
#[non_exhaustive]
pub enum Effect<P: JobPort> {
    /// Submit a job with this request.
    Submit(P::Request),
    /// Cancel the job of this submission.
    Cancel(P::Submission),
}

impl<P: JobPort> fmt::Debug for Effect<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Effect::Submit(request) => f.debug_tuple("Submit").field(request).finish(),
            Effect::Cancel(submission) => f.debug_tuple("Cancel").field(submission).finish(),
        }
    }
}

/// The execution port's answer to an effect, which the next round takes as
/// input.
pub enum Receipt<P: JobPort> {
    /// The submission was accepted with this identity; it becomes a
    /// [`Message::Accepted`](crate::Message::Accepted).
    Accepted(P::Submission),
    /// The submission was refused; it becomes a
    /// [`Message::Rejected`](crate::Message::Rejected).
    Rejected(P::Rejection),
    /// The answer to a cancellation, which goes only into the event
    /// handling record: a cancellation gives the application no receipt.
    Cancelled(P::CancelAnswer),
}

impl<P: JobPort> fmt::Debug for Receipt<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Receipt::Accepted(submission) => f.debug_tuple("Accepted").field(submission).finish(),
            Receipt::Rejected(rejection) => f.debug_tuple("Rejected").field(rejection).finish(),
            Receipt::Cancelled(answer) => f.debug_tuple("Cancelled").field(answer).finish(),
        }
    }
}

/// Hands `effects` to `port` in order and returns its receipts in the same
/// order, one for each effect: what a driver does between the two steps of
/// a round.
pub fn execute<P: JobPort>(port: &mut P, effects: Vec<Effect<P>>) -> Vec<Receipt<P>> {
    effects
        .into_iter()
        .map(|effect| match effect {
            Effect::Submit(request) => match port.submit(request) {
                Ok(submission) => Receipt::Accepted(submission),
                Err(rejection) => Receipt::Rejected(rejection),
            },
            Effect::Cancel(submission) => Receipt::Cancelled(port.cancel(&submission)),
        })
        .collect()
}
