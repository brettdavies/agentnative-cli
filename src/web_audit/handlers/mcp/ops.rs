//! The MCP op table: the per-op facts every family-dependent behavior in
//! the handler derives from, a mirror of the site's `MCP_OPS`. One
//! declaration rather than a set per behavior, so an op cannot join the
//! era family (and inherit its softening) by being absent from a
//! membership set.

/// Which classification branch judges the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// The row names a method the lane could be missing.
    Era,
    /// The row asks a question the lane has already proven it serves.
    Conformance,
}

/// Which protocol era's wire shape the row probes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Era {
    /// The 2025 initialize-and-session shape.
    Legacy,
    /// The header-routed, sessionless SEP-2243 shape.
    Modern,
}

/// One registry `with.op`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum McpOp {
    /// The legacy handshake: `initialize` under the pinned protocol version.
    Initialize,
    /// The legacy `tools/list` read.
    ToolsList,
    /// The legacy `resources/list` read.
    ResourcesList,
    /// A legacy method no server implements, judged on its refusal code.
    Error,
    /// A body that is not JSON at all.
    MalformedBody,
    /// A batch carrying a modern envelope, which the legacy lane rejects.
    BatchReject,
    /// A `tools/call` naming a tool outside any real catalog.
    UnknownTool,
    /// The proven `tools/list` under an Accept of JSON alone.
    AcceptJson,
    /// The proven `tools/list` under an Accept no transport can satisfy.
    AcceptUnsatisfiable,
    /// `server/discover`, the one modern-only method, so the row that
    /// decides whether the modern lane exists at all.
    ServerDiscover,
    /// The modern, header-routed `tools/list`.
    ModernToolsList,
    /// A modern method no server implements.
    ModernUnknownMethod,
    /// A modern request whose `_meta` omits the mandatory client capabilities.
    ModernClientcaps,
    /// A modern request whose routing header disagrees with its body method.
    ModernHeaderMismatch,
    /// A modern request claiming a protocol revision the lane must refuse.
    ModernVersionReject,
    /// A modern `resources/read` for a URI no server holds.
    ModernResourcesMiss,
}

/// The facts an op declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McpOpSpec {
    /// Which classification branch judges the answer.
    pub family: Family,
    /// Which era's wire shape the row probes.
    pub era: Era,
    /// Wire method for a modern era probe; header and body both read it.
    pub method: Option<&'static str>,
    /// Capability group the row reads, as named in its own lane's handshake.
    pub advertises: Option<&'static str>,
    /// The row whose answer decides the modern lane.
    pub discriminates: bool,
    /// The row passes on a refusal code rather than on a result.
    pub expects_refusal: bool,
    /// The row is judged on how the answer was framed.
    pub framed: bool,
}

const fn spec(family: Family, era: Era) -> McpOpSpec {
    McpOpSpec {
        family,
        era,
        method: None,
        advertises: None,
        discriminates: false,
        expects_refusal: false,
        framed: false,
    }
}

impl McpOp {
    /// Every op, in registry order.
    pub const ALL: [McpOp; 16] = [
        McpOp::Initialize,
        McpOp::ToolsList,
        McpOp::ResourcesList,
        McpOp::Error,
        McpOp::MalformedBody,
        McpOp::BatchReject,
        McpOp::UnknownTool,
        McpOp::AcceptJson,
        McpOp::AcceptUnsatisfiable,
        McpOp::ServerDiscover,
        McpOp::ModernToolsList,
        McpOp::ModernUnknownMethod,
        McpOp::ModernClientcaps,
        McpOp::ModernHeaderMismatch,
        McpOp::ModernVersionReject,
        McpOp::ModernResourcesMiss,
    ];

    /// The op for a registry spelling.
    pub fn parse(op: &str) -> Option<McpOp> {
        McpOp::ALL.into_iter().find(|o| o.as_str() == op)
    }

    /// The registry spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            McpOp::Initialize => "initialize",
            McpOp::ToolsList => "tools-list",
            McpOp::ResourcesList => "resources-list",
            McpOp::Error => "error",
            McpOp::MalformedBody => "malformed-body",
            McpOp::BatchReject => "batch-reject",
            McpOp::UnknownTool => "unknown-tool",
            McpOp::AcceptJson => "accept-json",
            McpOp::AcceptUnsatisfiable => "accept-unsatisfiable",
            McpOp::ServerDiscover => "server-discover",
            McpOp::ModernToolsList => "modern-tools-list",
            McpOp::ModernUnknownMethod => "modern-unknown-method",
            McpOp::ModernClientcaps => "modern-clientcaps",
            McpOp::ModernHeaderMismatch => "modern-header-mismatch",
            McpOp::ModernVersionReject => "modern-version-reject",
            McpOp::ModernResourcesMiss => "modern-resources-miss",
        }
    }

    /// The op's declared facts.
    pub const fn spec(self) -> McpOpSpec {
        match self {
            McpOp::Initialize => spec(Family::Era, Era::Legacy),
            McpOp::ToolsList => McpOpSpec {
                advertises: Some("tools"),
                ..spec(Family::Era, Era::Legacy)
            },
            McpOp::ResourcesList => McpOpSpec {
                advertises: Some("resources"),
                ..spec(Family::Era, Era::Legacy)
            },
            McpOp::Error => McpOpSpec {
                expects_refusal: true,
                ..spec(Family::Era, Era::Legacy)
            },
            McpOp::MalformedBody | McpOp::BatchReject | McpOp::UnknownTool => {
                spec(Family::Conformance, Era::Legacy)
            }
            McpOp::AcceptJson | McpOp::AcceptUnsatisfiable => McpOpSpec {
                framed: true,
                ..spec(Family::Conformance, Era::Legacy)
            },
            McpOp::ServerDiscover => McpOpSpec {
                method: Some("server/discover"),
                discriminates: true,
                ..spec(Family::Era, Era::Modern)
            },
            McpOp::ModernToolsList => McpOpSpec {
                method: Some("tools/list"),
                advertises: Some("tools"),
                ..spec(Family::Era, Era::Modern)
            },
            McpOp::ModernUnknownMethod
            | McpOp::ModernClientcaps
            | McpOp::ModernHeaderMismatch
            | McpOp::ModernVersionReject
            | McpOp::ModernResourcesMiss => spec(Family::Conformance, Era::Modern),
        }
    }

    /// Settled from the lane rather than probed on its own answer: every
    /// modern row except the one that discriminates the lane.
    pub fn modern_lane_dependent(self) -> bool {
        let s = self.spec();
        s.era == Era::Modern && !s.discriminates
    }

    /// Re-asked with the session when a stateful target refuses the
    /// sessionless probe: the code-judged legacy conformance rows.
    pub fn legacy_conformance(self) -> bool {
        let s = self.spec();
        s.family == Family::Conformance && s.era == Era::Legacy && !s.framed
    }

    /// Judged on a JSON-RPC code by the conformance branch.
    pub fn coded_conformance(self) -> bool {
        let s = self.spec();
        s.family == Family::Conformance && !s.framed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_families_partition_the_ops_as_on_the_site() {
        let names: Vec<&str> = McpOp::ALL.iter().map(|o| o.as_str()).collect();
        assert_eq!(names.len(), 16);
        for op in McpOp::ALL {
            assert_eq!(McpOp::parse(op.as_str()), Some(op));
        }
        assert_eq!(McpOp::parse("nope"), None);
        let dependent: Vec<&str> = McpOp::ALL
            .iter()
            .filter(|o| o.modern_lane_dependent())
            .map(|o| o.as_str())
            .collect();
        assert_eq!(
            dependent,
            [
                "modern-tools-list",
                "modern-unknown-method",
                "modern-clientcaps",
                "modern-header-mismatch",
                "modern-version-reject",
                "modern-resources-miss",
            ]
        );
        let legacy_conformance: Vec<&str> = McpOp::ALL
            .iter()
            .filter(|o| o.legacy_conformance())
            .map(|o| o.as_str())
            .collect();
        assert_eq!(
            legacy_conformance,
            ["malformed-body", "batch-reject", "unknown-tool"]
        );
        assert!(McpOp::ServerDiscover.spec().discriminates);
        assert!(McpOp::Error.spec().expects_refusal);
        assert!(McpOp::AcceptJson.spec().framed);
        assert!(!McpOp::AcceptJson.coded_conformance());
        assert!(McpOp::ModernResourcesMiss.coded_conformance());
    }
}
