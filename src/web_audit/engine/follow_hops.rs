//! A declared URL's reads through the request gate, a mirror of
//! `admitHop`, `fetchDeclared` and `readDeclaredDocuments` in the site's
//! `follow-requests.ts`: a declared URL may take one redirect hop, which
//! the guard validates and the gate charges before it is taken; a second
//! hop is never taken.

use super::discovery::resolve_url;
use super::follow_refusals::hop_refusal;
use super::follow_requests::{Fetched, Gate, ReadOptions, Step};
use super::follow_trail::{Declaration, Kind, Outcome, Settled};
use crate::web_audit::fetch::redirect::is_redirect;

impl Gate<'_> {
    /// Where a first answer leads: itself, or the one redirect hop the
    /// guard, the host slots and the request cap admit, as a GET still to
    /// send or the way the hop settled.
    fn hop(&mut self, d: &Declaration, first: Fetched, opts: ReadOptions) -> Result<Fetched, Step> {
        let location = first.response.headers.get("location").map(str::to_string);
        let (Some(location), Some(true)) = (location, first.response.status.map(is_redirect))
        else {
            return Ok(first);
        };
        let hop = resolve_url(&d.url, &location);
        if let Some(refused) = hop_refusal(&hop, d.kind, self.fetch, self.target_locality) {
            return Err(Step::Settled(refused));
        }
        if let Err(cause) = self.enter(&hop) {
            return Err(Step::Settled(Settled::budget(cause)));
        }
        Err(self.admit(&hop, opts))
    }

    /// GETs a declared URL, taking at most one admitted redirect hop.
    pub fn fetch_declared(
        &mut self,
        d: &Declaration,
        opts: ReadOptions,
    ) -> Result<Fetched, Settled> {
        let first = self.admit(&d.url, opts);
        let first = self.send(vec![first]).pop().expect("one read")?;
        match self.hop(d, first, opts) {
            Ok(landed) => Ok(landed),
            Err(step) => {
                let landed = self.send(vec![step]).pop().expect("one read")?;
                second_redirect(d.kind, landed)
            }
        }
    }

    /// Reads declared documents side by side. Each first GET is admitted in
    /// declaration order before it starts, and each redirect hop in
    /// declaration order once every first GET has answered, so host slots
    /// and the request cap never go to whichever host answers first.
    pub fn read_declared(
        &mut self,
        declarations: &[Declaration],
        upfront: impl Fn(&Declaration) -> Option<Settled>,
        opts: ReadOptions,
    ) -> Vec<Result<Fetched, Settled>> {
        let firsts: Vec<Step> = declarations
            .iter()
            .map(|d| match upfront(d) {
                Some(settled) => Step::Settled(settled),
                None => match self.enter(&d.url) {
                    Ok(()) => self.admit(&d.url, opts),
                    Err(cause) => Step::Settled(Settled::budget(cause)),
                },
            })
            .collect();
        let mut landings: Vec<Option<Result<Fetched, Settled>>> = Vec::new();
        let mut hops: Vec<Step> = Vec::new();
        for (d, first) in declarations.iter().zip(self.send(firsts)) {
            match first.map(|fetched| self.hop(d, fetched, opts)) {
                Ok(Err(hop)) => {
                    landings.push(None);
                    hops.push(hop);
                }
                Ok(Ok(fetched)) => landings.push(Some(Ok(fetched))),
                Err(settled) => landings.push(Some(Err(settled))),
            }
        }
        let mut hop_reads = self.send(hops).into_iter();
        declarations
            .iter()
            .zip(landings)
            .map(|(d, landing)| match landing {
                Some(read) => read,
                None => hop_reads
                    .next()
                    .expect("one read per hop")
                    .and_then(|landed| second_redirect(d.kind, landed)),
            })
            .collect()
    }
}

/// An endpoint whose hop redirects again is refused; a document's second
/// redirect is its answer.
fn second_redirect(kind: Kind, landed: Fetched) -> Result<Fetched, Settled> {
    if kind == Kind::McpEndpoint && landed.response.status.is_some_and(is_redirect) {
        return Err(Settled::landed(
            Some(landed.url),
            Outcome::ReciprocityRefused,
        ));
    }
    Ok(landed)
}
