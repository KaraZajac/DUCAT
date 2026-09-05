use super::*;

#[derive(Debug)]
pub struct AnswerContext {
    /// How long it took to get this answer
    pub latency: TimestampDuration,
    /// The context of the waitable reply that produced this answer
    pub waitable_reply_context: WaitableReplyContext,
}

#[derive(Debug)]
pub struct Answer<T> {
    /// The context of the answer
    pub answer_context: AnswerContext,
    /// The answer itself
    pub answer: T,
}
impl<T> Answer<T> {
    pub fn new(answer_context: AnswerContext, answer: T) -> Self {
        Self {
            answer_context,
            answer,
        }
    }
}
