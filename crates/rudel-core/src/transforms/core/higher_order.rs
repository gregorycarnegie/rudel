use super::IntoPattern;
use crate::pattern::Pattern;
use crate::value::Value;

impl Pattern {
    // -- Higher-order combinators ------------------------------------------

    /// Apply `f` to a layered copy and stack it on top (`superimpose`).
    pub fn superimpose<F>(&self, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.stack_with(&f(self))
    }

    /// Layer copies produced by each function on top of this pattern (`layer`).
    pub fn layer<F>(&self, funcs: &[F]) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let pats: Vec<Pattern> = funcs.iter().map(|f| f(self)).collect();
        crate::pattern::stack(&pats)
    }

    /// Offset a copy by `time` cycles, transform it with `f`, and stack it
    /// (`off`).
    pub fn off<F>(&self, time: impl IntoPattern, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let shifted = self.late(time);
        self.stack_with(&f(&shifted))
    }

    /// Apply `f` every `n`th cycle, on the first cycle of each group
    /// (`every`/`firstOf`).
    pub fn every<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        if n <= 0 {
            return self.clone();
        }
        let mut pats = Vec::with_capacity(n as usize);
        pats.push(f(self));
        for _ in 1..n {
            pats.push(self.clone());
        }
        crate::pattern::slowcat_prime(&pats)
    }

    /// Alias for [`every`](Self::every) (`firstOf`).
    pub fn first_of<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.every(n, f)
    }

    /// Apply `f` every `n`th cycle, on the *last* cycle of each group
    /// (`lastOf`).
    pub fn last_of<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        if n <= 0 {
            return self.clone();
        }
        let mut pats: Vec<Pattern> = (0..n - 1).map(|_| self.clone()).collect();
        pats.push(f(self));
        crate::pattern::slowcat_prime(&pats)
    }

    /// Place an already-transformed pattern on the first (`last = false`,
    /// `every`/`firstOf`) or last (`last = true`, `lastOf`) cycle of each group
    /// of `n`. Shared by the patternified Koto bindings, which apply the Koto
    /// callback eagerly (the VM can't run in the query path), so the transform
    /// is supplied as a concrete pattern rather than a closure.
    pub fn every_cycles(&self, transformed: &Pattern, n: i64, last: bool) -> Pattern {
        if n <= 0 {
            return self.clone();
        }
        let mut pats: Vec<Pattern> = Vec::with_capacity(n as usize);
        if last {
            for _ in 0..n - 1 {
                pats.push(self.clone());
            }
            pats.push(transformed.clone());
        } else {
            pats.push(transformed.clone());
            for _ in 1..n {
                pats.push(self.clone());
            }
        }
        crate::pattern::slowcat_prime(&pats)
    }

    /// [`every_cycles`](Self::every_cycles) with a patternified cycle count, so
    /// `every("<2 4>", f)` samples `n` once per cycle (mirroring Strudel's
    /// `register` patternification of the count argument).
    pub fn every_pat(&self, n: impl IntoPattern, transformed: Pattern, last: bool) -> Pattern {
        let n = n.into_pattern();
        if let Some(v) = &n.pure_value {
            return self.every_cycles(&transformed, v.to_frac().to_f64() as i64, last);
        }
        let pat = self.clone();
        n.fmap(move |nv| {
            let count = nv.as_f64().unwrap_or(0.0) as i64;
            Value::Pat(Box::new(pat.every_cycles(&transformed, count, last)))
        })
        .inner_join()
    }
}
