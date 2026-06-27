// state.rs - ported from strudel/packages/core/state.mjs
// Copyright (C) 2022 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{timespan::TimeSpan, value::ValueMap};

/// Query state: the span being queried plus ambient controls.
#[derive(Clone, Debug)]
pub struct State {
    pub span: TimeSpan,
    pub controls: ValueMap,
}

impl State {
    pub fn new(span: TimeSpan) -> Self {
        State {
            span,
            controls: ValueMap::new(),
        }
    }

    pub fn with_controls(span: TimeSpan, controls: ValueMap) -> Self {
        State { span, controls }
    }

    pub fn set_span(&self, span: TimeSpan) -> State {
        State {
            span,
            controls: self.controls.clone(),
        }
    }

    pub fn with_span(&self, f: impl Fn(TimeSpan) -> TimeSpan) -> State {
        self.set_span(f(self.span))
    }
}
