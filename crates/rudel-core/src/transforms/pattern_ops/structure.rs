use crate::{
    fraction::Frac,
    hap::{Context, Hap},
    pattern::{Pattern, fastcat, pure, silence, slowcat},
    timespan::TimeSpan,
    transforms::IntoPattern,
    value::Value,
};
use std::sync::Arc;

impl Pattern {
    /// Build a structure pattern from cycle divisions (`beat`): place this
    /// pattern's value at division `t` of `div` slices per cycle. `t` and `div`
    /// are patternified, so `s("bd").beat("0,7,10", 16)` stacks three beats.
    pub fn beat(&self, t: impl IntoPattern, div: impl IntoPattern) -> Pattern {
        let pat = self.clone();
        let div_pat = div.into_pattern();
        t.into_pattern().inner_bind(move |tv| {
            let pat = pat.clone();
            let t = tv.to_frac();
            div_pat.inner_bind(move |dv| beat_once(pat.clone(), t, dv.to_frac()))
        })
    }

    /// `collect`: group simultaneous (congruent) haps into a single hap whose
    /// value is a [`Value::List`] of the grouped values, preserving order.
    pub fn collect(&self) -> Pattern {
        self.with_haps(|haps, _| {
            let mut groups: Vec<(Option<TimeSpan>, TimeSpan, Vec<Value>, Context)> = Vec::new();
            for hap in haps {
                match groups
                    .iter_mut()
                    .find(|(w, p, _, _)| *w == hap.whole && *p == hap.part)
                {
                    Some(group) => group.2.push(hap.value),
                    None => groups.push((hap.whole, hap.part, vec![hap.value], hap.context)),
                }
            }
            groups
                .into_iter()
                .map(|(whole, part, values, ctx)| {
                    Hap::new(whole, part, Value::List(values)).set_context(ctx)
                })
                .collect()
        })
    }

    /// `arpWith`: collect simultaneous notes into chords, then for each chord
    /// build a pattern with `func` (given the chord's values) and play it within
    /// the chord's timespan.
    pub fn arp_with<F>(&self, func: F) -> Pattern
    where
        F: Fn(&[Value]) -> Pattern + Send + Sync + 'static,
    {
        let func = Arc::new(func);
        self.collect().inner_bind(move |list_val| {
            let notes = match list_val {
                Value::List(v) => v,
                other => vec![other],
            };
            if notes.is_empty() {
                silence()
            } else {
                func(&notes)
            }
        })
    }

    /// `arp`: arpeggiate chords by selecting their notes with an index pattern
    /// (`haps[i % len]`), e.g. `note("<[c,eb,g]>").arp("0 1 2 1")`.
    pub fn arp(&self, indices: impl IntoPattern) -> Pattern {
        let indices = indices.into_pattern();
        self.arp_with(move |notes| {
            let notes = Arc::new(notes.to_vec());
            indices.clone().fmap(move |idx| {
                let i = idx.as_f64().unwrap_or(0.0).max(0.0) as usize;
                notes[i % notes.len()].clone()
            })
        })
    }

    /// `arpeggiate`: play each chord's notes in sequence across its timespan.
    pub fn arpeggiate(&self) -> Pattern {
        self.arp_with(|notes| {
            let pats: Vec<Pattern> = notes.iter().cloned().map(pure).collect();
            fastcat(&pats)
        })
    }

    /// `shuffle(n)`: slice the pattern into `n` parts and play them in a random
    /// order; each part plays exactly once per cycle.
    pub fn shuffle(&self, n: i64) -> Pattern {
        rearrange_with(crate::signal::randrun(n), n, self)
    }

    /// `scramble(n)`: slice the pattern into `n` parts and play parts picked at
    /// random — unlike [`shuffle`](Self::shuffle), parts may repeat or be
    /// skipped within a cycle.
    pub fn scramble(&self, n: i64) -> Pattern {
        rearrange_with(crate::signal::irand(n)._segment(Frac::int(n)), n, self)
    }

    /// `tour`: insert this pattern into a list of patterns, first at the end,
    /// then moving backwards through the list on successive repetitions, all
    /// concatenated stepwise into a single cycle.
    pub fn tour(&self, many: &[Pattern]) -> Pattern {
        let len = many.len();
        let mut pats: Vec<Pattern> = Vec::new();
        for i in 0..len {
            pats.extend_from_slice(&many[..len - i]);
            pats.push(self.clone());
            pats.extend_from_slice(&many[len - i..]);
        }
        pats.push(self.clone());
        pats.extend_from_slice(many);
        crate::pattern::stepcat(&pats)
    }
}

/// Slice `pat` into `n` parts and rearrange them per the integer signal `ipat`
/// (signal.mjs `_rearrangeWith`; shared by `shuffle`/`scramble`).
fn rearrange_with(ipat: Pattern, n: i64, pat: &Pattern) -> Pattern {
    if n <= 0 {
        return silence();
    }
    let parts: Vec<Pattern> = (0..n)
        .map(|i| pat.zoom(Frac::int(i) / Frac::int(n), Frac::int(i + 1) / Frac::int(n)))
        .collect();
    ipat.inner_bind(move |v| {
        let i = (v.as_f64().unwrap_or(0.0) as i64).rem_euclid(n) as usize;
        parts[i].repeat_cycles(n)._fast(Frac::int(n))
    })
}

/// 'zip' the steps of the given patterns together into one dense cycle:
/// step 1 of each pattern, then step 2 of each, … Patterns without step
/// metadata are ignored (`zip`).
pub fn zip(pats: &[Pattern]) -> Pattern {
    let stepped: Vec<&Pattern> = pats
        .iter()
        .filter(|p| p.steps.is_some_and(|s| s > Frac::zero()))
        .collect();
    let Some(steps) = stepped
        .iter()
        .filter_map(|p| p.steps)
        .reduce(|a, b| a.lcm(b))
    else {
        return silence();
    };
    let slowed: Vec<Pattern> = stepped
        .iter()
        .map(|p| p._slow(p.steps.unwrap_or_else(Frac::one)))
        .collect();
    slowcat(&slowed)._fast(steps).set_steps(Some(steps))
}

/// `stepalt`: alternate stepwise between groups, taking one element from each
/// group per cycle and cycling within each group independently. The result's
/// step count is the sum of the chosen patterns' steps.
pub fn stepalt(groups: &[Vec<Pattern>]) -> Pattern {
    if groups.is_empty() {
        return silence();
    }
    // Repeat for LCM(group lengths) cycles so every group realigns.
    let cycles = groups
        .iter()
        .map(|g| Frac::int(g.len().max(1) as i64))
        .reduce(|a, b| a.lcm(b))
        .unwrap_or_else(Frac::one);
    let cycles = (cycles.to_f64().round() as i64).max(1);
    let mut chosen: Vec<Pattern> = Vec::new();
    for cycle in 0..cycles {
        for group in groups {
            if group.is_empty() {
                continue;
            }
            chosen.push(group[(cycle as usize) % group.len()].clone());
        }
    }
    crate::pattern::stepcat(&chosen)
}

/// One beat: place `pat`'s value into the `[t/div, (t+1)/div]` slice of the
/// cycle (Strudel's `__beat` with `innerJoin`). `t` is taken modulo `div`.
fn beat_once(pat: Pattern, t: Frac, div: Frac) -> Pattern {
    if div <= Frac::zero() {
        return silence();
    }
    // Floored modulo (Fraction.mod), so a position beyond `div` wraps.
    let t = t - (t / div).floor() * div;
    let b = t / div;
    let e = (t + Frac::one()) / div;
    pat.fmap(move |x| Value::Pat(Box::new(pure(x)._compress(b, e))))
        .inner_join()
}
