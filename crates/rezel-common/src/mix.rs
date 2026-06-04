use std::fmt;
use std::sync::Arc;

use crate::parse::run_to_completion;
use crate::{
    Input, IterMode, MountedTree, ParseError, ParseErrorKind, ParseRequest, ParseWrapper, Parser,
    PartialParse, SyntaxNode, TextRange, TextSize, Tree, TreeCursor, mounted_prop,
};

/// Result of an overlay predicate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayMatch {
    /// Exclude this descendant.
    None,
    /// Include the descendant's complete range.
    Node,
    /// Include an explicit absolute input range.
    Range(TextRange),
}

/// Mixed-language overlay selection.
#[derive(Clone)]
pub enum Overlay {
    /// The inner tree replaces the complete host node.
    Replace,
    /// Parse explicit absolute input ranges and mount them as an overlay.
    Ranges(Arc<[TextRange]>),
    /// Collect overlay ranges from descendants of the host node.
    Predicate(Arc<dyn Fn(&SyntaxNode) -> OverlayMatch + Send + Sync>),
}

impl fmt::Debug for Overlay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Replace => formatter.write_str("Replace"),
            Self::Ranges(ranges) => formatter.debug_tuple("Ranges").field(ranges).finish(),
            Self::Predicate(_) => formatter.write_str("Predicate(..)"),
        }
    }
}

/// One nested parser selected by a mixed-parser callback.
#[derive(Clone)]
pub struct NestedParse {
    /// Parser for the inner region.
    pub parser: Arc<dyn Parser>,
    /// Replacement or overlay selection.
    pub overlay: Overlay,
    /// Whether the nested region is surrounded by bracket tokens.
    pub bracketed: bool,
}

impl fmt::Debug for NestedParse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NestedParse")
            .field("parser", &"<parser>")
            .field("overlay", &self.overlay)
            .field("bracketed", &self.bracketed)
            .finish()
    }
}

/// Callback used to select nested parsers from a completed outer tree.
pub type MixedParseSpec = Arc<dyn Fn(&SyntaxNode, &dyn Input) -> Option<NestedParse> + Send + Sync>;

/// Build a mixed-parse wrapper.
#[must_use]
pub fn parse_mixed(spec: MixedParseSpec) -> ParseWrapper {
    Arc::new(move |base, request| {
        Box::new(MixedParse {
            base: Some(base),
            request,
            spec: Arc::clone(&spec),
            result: None,
            stopped_at: None,
        })
    })
}

struct MixedParse {
    base: Option<Box<dyn PartialParse>>,
    request: ParseRequest,
    spec: MixedParseSpec,
    result: Option<Tree>,
    stopped_at: Option<TextSize>,
}

impl PartialParse for MixedParse {
    fn advance(&mut self) -> Result<Option<Tree>, ParseError> {
        if let Some(result) = self.result.take() {
            return Ok(Some(result));
        }
        let Some(base) = self.base.as_mut() else {
            return Ok(None);
        };
        let Some(tree) = base.advance()? else {
            return Ok(None);
        };
        self.base = None;
        Ok(Some(mount_inner_parses(&tree, &self.request, &self.spec)?))
    }

    fn parsed_position(&self) -> TextSize {
        self.base
            .as_ref()
            .map_or_else(|| self.request.input().len(), |base| base.parsed_position())
    }

    fn stop_at(&mut self, position: TextSize) -> Result<(), ParseError> {
        if position > self.request.input().len() {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(position),
                "mixed parse stop position is outside the input",
            ));
        }
        if self.stopped_at.is_some_and(|previous| position > previous) {
            return Err(ParseError::new(
                ParseErrorKind::Input,
                Some(position),
                "a parse stop position cannot move forward",
            ));
        }
        self.stopped_at = Some(position);
        if let Some(base) = self.base.as_mut() {
            base.stop_at(position)?;
        }
        Ok(())
    }

    fn stopped_at(&self) -> Option<TextSize> {
        self.stopped_at
    }
}

fn mount_inner_parses(
    tree: &Tree,
    request: &ParseRequest,
    spec: &MixedParseSpec,
) -> Result<Tree, ParseError> {
    let (tree, jobs) = collect_inner_parses(tree, request, spec)?;
    for job in jobs {
        let inner_tree = if job.ranges.is_empty() {
            job.parser.parse("")?
        } else {
            let inner_request = ParseRequest::ranges(Arc::clone(request.input()), job.ranges)?;
            run_to_completion(job.parser.create_parse(inner_request)?)?
        };
        job.target.set_prop(
            mounted_prop(),
            MountedTree {
                tree: inner_tree,
                overlay: job.overlay,
                parser: job.parser,
                bracketed: job.bracketed,
            },
        );
    }
    Ok(tree)
}

struct InnerParseJob {
    parser: Arc<dyn Parser>,
    ranges: Vec<TextRange>,
    overlay: Option<Arc<[TextRange]>>,
    bracketed: bool,
    target: Tree,
}

struct ActiveOverlay {
    parser: Arc<dyn Parser>,
    predicate: Arc<dyn Fn(&SyntaxNode) -> OverlayMatch + Send + Sync>,
    ranges: Vec<TextRange>,
    bracketed: bool,
    target: Tree,
    start: TextSize,
    insertion_index: usize,
    depth: usize,
}

struct CoveredRanges {
    ranges: Vec<TextRange>,
    job_index: usize,
    depth: usize,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Coverage {
    None,
    Partial,
    Full,
}

fn collect_inner_parses(
    tree: &Tree,
    request: &ParseRequest,
    spec: &MixedParseSpec,
) -> Result<(Tree, Vec<InnerParseJob>), ParseError> {
    let mut jobs: Vec<Option<InnerParseJob>> = Vec::new();
    let mut overlays: Vec<ActiveOverlay> = Vec::new();
    let mut covered: Vec<CoveredRanges> = Vec::new();
    let mut cursor = tree.cursor(IterMode::INCLUDE_ANONYMOUS | IterMode::IGNORE_MOUNTS);
    'scan: loop {
        let node = cursor.node();
        let coverage = covered
            .last()
            .map_or(Coverage::None, |state| cover(&state.ranges, node.range()));

        let enter = if coverage == Coverage::None {
            visit_uncovered_node(
                &mut cursor,
                &node,
                request,
                spec,
                &mut jobs,
                &mut overlays,
                &mut covered,
            )?
        } else {
            coverage == Coverage::Partial
        };

        if enter && cursor.first_child() {
            if let Some(active) = overlays.last_mut() {
                active.depth += 1;
            }
            if let Some(state) = covered.last_mut() {
                state.depth += 1;
            }
            continue;
        }

        finish_zero_depth_scopes(&mut overlays, &mut covered, &mut jobs, request)?;
        loop {
            if cursor.next_sibling() {
                break;
            }
            if !cursor.parent() {
                break 'scan;
            }
            leave_scopes(&mut overlays, &mut covered, &mut jobs, request)?;
        }
    }
    Ok((cursor.root_tree(), jobs.into_iter().flatten().collect()))
}

fn visit_uncovered_node(
    cursor: &mut TreeCursor,
    node: &SyntaxNode,
    request: &ParseRequest,
    spec: &MixedParseSpec,
    jobs: &mut Vec<Option<InnerParseJob>>,
    overlays: &mut Vec<ActiveOverlay>,
    covered: &mut Vec<CoveredRanges>,
) -> Result<bool, ParseError> {
    let node_range = node.range();
    let node_from = node.from();
    let nested = if node.node_type().is_anonymous() {
        None
    } else {
        spec(node, request.input().as_ref())
    };
    let nested = nested
        .filter(|nested| !node_range.is_empty() || matches!(nested.overlay, Overlay::Replace));
    let Some(nested) = nested else {
        if let Some(active) = overlays.last_mut() {
            let candidate = match (active.predicate)(node) {
                OverlayMatch::None => None,
                OverlayMatch::Node => Some(node.range()),
                OverlayMatch::Range(range) => Some(range),
            };
            if let Some(candidate) = candidate
                && !candidate.is_empty()
            {
                push_range(&mut active.ranges, candidate);
            }
        }
        return Ok(true);
    };

    let was_packed = cursor.tree().is_none();
    let target = cursor.materialize_current(|old, new| {
        replace_materialized_targets(jobs, overlays, covered, old, new);
    });
    if was_packed {
        if let Some(active) = overlays.last_mut() {
            active.depth += 1;
        }
        if let Some(state) = covered.last_mut() {
            state.depth += 1;
        }
    }
    match nested.overlay {
        Overlay::Replace => {
            let requested = if node_range.is_empty() {
                Vec::new()
            } else {
                vec![node_range]
            };
            let ranges = intersect_ranges(request.selected_ranges(), &requested);
            jobs.push(Some(InnerParseJob {
                parser: nested.parser,
                ranges,
                overlay: None,
                bracketed: nested.bracketed,
                target,
            }));
            Ok(false)
        }
        Overlay::Ranges(ranges) => {
            if !ranges.is_empty() {
                validate_inner_ranges(request.input().len(), &ranges)?;
                let parse_ranges = intersect_ranges(request.selected_ranges(), &ranges);
                if !parse_ranges.is_empty() {
                    let job_index = jobs.len();
                    jobs.push(Some(InnerParseJob {
                        parser: nested.parser,
                        ranges: parse_ranges.clone(),
                        overlay: Some(relative_ranges(&ranges, node_from)),
                        bracketed: nested.bracketed,
                        target,
                    }));
                    covered.push(CoveredRanges {
                        ranges: parse_ranges,
                        job_index,
                        depth: 0,
                    });
                }
            }
            Ok(true)
        }
        Overlay::Predicate(predicate) => {
            let insertion_index = jobs.len();
            jobs.push(None);
            overlays.push(ActiveOverlay {
                parser: nested.parser,
                predicate,
                ranges: Vec::new(),
                bracketed: nested.bracketed,
                target,
                start: node_from,
                insertion_index,
                depth: 0,
            });
            Ok(true)
        }
    }
}

fn replace_materialized_targets(
    jobs: &mut [Option<InnerParseJob>],
    overlays: &mut [ActiveOverlay],
    covered: &[CoveredRanges],
    old: &Tree,
    new: &Tree,
) {
    for overlay in overlays {
        if overlay.target.same_identity(old) {
            overlay.target = new.clone();
        }
    }
    for state in covered {
        let Some(job) = jobs[state.job_index].as_mut() else {
            continue;
        };
        if job.target.same_identity(old) {
            job.target = new.clone();
        }
    }
}

fn relative_ranges(ranges: &[TextRange], start: TextSize) -> Arc<[TextRange]> {
    ranges
        .iter()
        .map(|range| TextRange::new(range.start() - start, range.end() - start))
        .collect::<Vec<_>>()
        .into()
}

fn push_range(ranges: &mut Vec<TextRange>, candidate: TextRange) {
    if let Some(previous) = ranges.last_mut()
        && previous.end() == candidate.start()
    {
        *previous = TextRange::new(previous.start(), candidate.end());
    } else {
        ranges.push(candidate);
    }
}

fn finish_zero_depth_scopes(
    overlays: &mut Vec<ActiveOverlay>,
    covered: &mut Vec<CoveredRanges>,
    jobs: &mut [Option<InnerParseJob>],
    request: &ParseRequest,
) -> Result<(), ParseError> {
    if overlays.last().is_some_and(|active| active.depth == 0) {
        let active = overlays.pop().expect("an active overlay is present");
        finish_overlay(active, jobs, request)?;
    }
    if covered.last().is_some_and(|state| state.depth == 0) {
        covered.pop();
    }
    Ok(())
}

fn leave_scopes(
    overlays: &mut Vec<ActiveOverlay>,
    covered: &mut Vec<CoveredRanges>,
    jobs: &mut [Option<InnerParseJob>],
    request: &ParseRequest,
) -> Result<(), ParseError> {
    if let Some(active) = overlays.last_mut() {
        active.depth -= 1;
        if active.depth == 0 {
            let active = overlays.pop().expect("an active overlay is present");
            finish_overlay(active, jobs, request)?;
        }
    }
    if let Some(state) = covered.last_mut() {
        state.depth -= 1;
        if state.depth == 0 {
            covered.pop();
        }
    }
    Ok(())
}

fn finish_overlay(
    active: ActiveOverlay,
    jobs: &mut [Option<InnerParseJob>],
    request: &ParseRequest,
) -> Result<(), ParseError> {
    if active.ranges.is_empty() {
        return Ok(());
    }
    validate_inner_ranges(request.input().len(), &active.ranges)?;
    let parse_ranges = intersect_ranges(request.selected_ranges(), &active.ranges);
    if parse_ranges.is_empty() {
        return Ok(());
    }
    let job = InnerParseJob {
        parser: active.parser,
        ranges: parse_ranges,
        overlay: Some(relative_ranges(&active.ranges, active.start)),
        bracketed: active.bracketed,
        target: active.target,
    };
    jobs[active.insertion_index] = Some(job);
    Ok(())
}

fn cover(ranges: &[TextRange], node: TextRange) -> Coverage {
    for range in ranges {
        if range.start() >= node.end() {
            break;
        }
        if range.end() > node.start() {
            return if range.start() <= node.start() && range.end() >= node.end() {
                Coverage::Full
            } else {
                Coverage::Partial
            };
        }
    }
    Coverage::None
}

fn intersect_ranges(selected: &[TextRange], requested: &[TextRange]) -> Vec<TextRange> {
    let mut intersections = Vec::new();
    let mut selected_index = 0;
    let mut requested_index = 0;
    while selected_index < selected.len() && requested_index < requested.len() {
        let left = selected[selected_index];
        let right = requested[requested_index];
        let start = left.start().max(right.start());
        let end = left.end().min(right.end());
        if start < end {
            intersections.push(TextRange::new(start, end));
        }
        if left.end() <= right.end() {
            selected_index += 1;
        } else {
            requested_index += 1;
        }
    }
    intersections
}

fn validate_inner_ranges(input_length: TextSize, ranges: &[TextRange]) -> Result<(), ParseError> {
    if ranges.is_empty()
        || ranges.iter().any(|range| range.is_empty())
        || ranges.iter().any(|range| range.end() > input_length)
        || ranges
            .windows(2)
            .any(|pair| pair[0].end() > pair[1].start())
    {
        return Err(ParseError::new(
            ParseErrorKind::Input,
            ranges.first().map(|range| range.start()),
            "invalid mixed parse ranges",
        ));
    }
    Ok(())
}
