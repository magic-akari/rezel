use rezel_common::{IterMode, SyntaxLanguage, SyntaxNode, TextRange, TextSize, Tree, TypedNode};

use super::build_constraint::go_version;
use super::model::{
    AstBuilder, AstError, AstNodeId, GoAst, GoAstField, GoAstKind, GoChanDirection, GoSourceRange,
    GoToken,
};
use crate::syntax::{
    GoAssignmentOperand, GoChannelTypeDirection, GoChannelTypeLayer, GoChannelTypeShape,
    GoClauseShape, GoConversionFunction, GoExpressionShape, GoGeneralDeclaration,
    GoGeneralDeclarationKind, GoSignature, GoStatementListItem, GoStatementListShape, GoValueSpec,
    assignment_shape, channel_type_shape, conversion_shape, expression_shape, for_clause_shape,
    index_shape, range_shape, slice_shape, statement_list, type_parameter_shape,
};
use crate::typed::{
    GoArrayType, GoBlock, GoCallExpr, GoCase, GoChannelType, GoConstDecl, GoConversion,
    GoDeclaration, GoDeferStatement, GoElement, GoExprStatement, GoFallthroughStatement,
    GoFieldDecl, GoForClause, GoForStatement, GoFunctionDecl, GoFunctionLiteral, GoFunctionType,
    GoGoStatement, GoGotoStatement, GoIfStatement, GoImportSpec, GoIncDecStatement,
    GoInterfaceBody, GoInterfaceType, GoKind, GoLabeledStatement, GoLanguage, GoMapType,
    GoMethodDecl, GoMethodElem, GoParameter, GoParameterizedExpr, GoParameterizedType,
    GoParameters, GoParenthesizedExpr, GoParenthesizedType, GoPointerType, GoQualifiedType,
    GoReturnStatement, GoSelectBlock, GoSelectStatement, GoSelectorExpr, GoSendStatement,
    GoSliceType, GoSourceFile, GoSpec, GoSpecList, GoStructBody, GoStructType, GoSwitchBlock,
    GoSwitchStatement, GoTypeAssertion, GoTypeDecl, GoTypeElem, GoTypeParam, GoTypeParams,
    GoTypeSpec, GoTypeSwitchStatement, GoTypedLiteral, GoUnderlyingType, GoVarDecl,
};

impl GoAst {
    /// Lower one strict Go CST to the public Go 1.26 `go/ast` model.
    ///
    /// # Errors
    ///
    /// Returns [`AstError::RecoveryTree`] when the CST contains recovery
    /// nodes, and an invariant error when the strict CST cannot be projected
    /// through the checked Go AST mapping.
    pub fn lower(tree: &Tree, source: &str) -> Result<Self, AstError> {
        Lowerer::new(source)?.lower(tree)
    }
}

#[derive(Clone, Copy)]
struct CommentGroup {
    node: AstNodeId,
    from: TextSize,
    to: TextSize,
}

#[derive(Clone, Copy)]
struct LoweredComment {
    node: AstNodeId,
    from: TextSize,
    to: TextSize,
}

struct PendingCommentGroup {
    comments: Vec<LoweredComment>,
    max_line_breaks: usize,
}

#[derive(Clone, Copy, Default)]
struct FieldComments {
    doc: Option<AstNodeId>,
    line: Option<AstNodeId>,
}

struct LoweredForClause {
    init: Option<AstNodeId>,
    condition: Option<AstNodeId>,
    post: Option<AstNodeId>,
}

struct Lowerer<'source> {
    source: &'source str,
    source_end: TextSize,
    ast: AstBuilder,
    comments: Vec<CommentGroup>,
    go_version: String,
}

enum GoExpressionAction {
    Visit(SyntaxNode),
    LowerUnary(SyntaxNode),
    LowerBinary(SyntaxNode),
    LowerPostfix(SyntaxNode),
}

impl<'source> Lowerer<'source> {
    fn new(source: &'source str) -> Result<Self, AstError> {
        let source_end = TextSize::try_from(source.len()).map_err(|_| AstError::SourceTooLarge)?;
        Ok(Self {
            source,
            source_end,
            ast: AstBuilder::new(),
            comments: Vec::new(),
            go_version: String::new(),
        })
    }

    fn lower(mut self, tree: &Tree) -> Result<GoAst, AstError> {
        reject_recovery_tree(tree)?;
        let source_file = GoSourceFile::downcast_from(tree.top_node()).map_err(|_| {
            AstError::InconsistentCst {
                context: "Go source",
                expected: "SourceFile",
            }
        })?;
        let package = source_file
            .package_clause()
            .ok_or(AstError::InconsistentCst {
                context: "SourceFile",
                expected: "PackageClause",
            })?;
        let package_name = package.name().ok_or(AstError::InconsistentCst {
            context: "PackageClause",
            expected: "DefName",
        })?;
        let package_keyword = package.package_token().ok_or(AstError::InconsistentCst {
            context: "PackageClause",
            expected: "package token",
        })?;
        let root = self.ast.push_node(
            GoAstKind::File,
            GoSourceRange::new(
                Some(package.syntax().from()),
                Some(package_name.syntax().to()),
            ),
        )?;

        self.lower_comments(tree, package.syntax().from())?;
        let doc = self.leading_comment(package_keyword.from());
        let name = self.lower_ident(package_name.syntax())?;
        let mut declarations = source_file
            .imports()
            .map(|declaration| self.lower_gen_decl(GoGeneralDeclaration::Import(&declaration)))
            .collect::<Result<Vec<_>, _>>()?;
        declarations.extend(
            source_file
                .declarations()
                .map(|declaration| self.lower_declaration(&declaration))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let imports = self.collect_imports(&declarations)?;
        let comment_nodes = self
            .comments
            .iter()
            .map(|comment| comment.node)
            .collect::<Vec<_>>();
        let end = declarations
            .last()
            .and_then(|id| self.ast_node_end(*id))
            .unwrap_or(package_name.syntax().to());
        self.ast.set_range(
            root,
            GoSourceRange::new(Some(package.syntax().from()), Some(end)),
        )?;

        self.ast.push_node_field(root, GoAstField::Doc, doc)?;
        self.ast
            .push_position_field(root, GoAstField::Package, Some(package_keyword.from()))?;
        self.ast
            .push_node_field(root, GoAstField::Name, Some(name))?;
        self.ast
            .push_nodes_field(root, GoAstField::Decls, &declarations)?;
        self.ast
            .push_position_field(root, GoAstField::FileStart, Some(TextSize::from(0)))?;
        self.ast
            .push_position_field(root, GoAstField::FileEnd, Some(self.source_end))?;
        self.ast
            .push_nodes_field(root, GoAstField::Imports, &imports)?;
        self.ast
            .push_nodes_field(root, GoAstField::Comments, &comment_nodes)?;
        self.ast
            .push_string_field(root, GoAstField::GoVersion, &self.go_version)?;
        self.ast.finish(root)
    }

    fn ast_node_end(&self, node: AstNodeId) -> Option<TextSize> {
        self.ast
            .nodes
            .get(node.index())
            .and_then(|node| node.source_range().end())
    }

    fn ast_node_start(&self, node: AstNodeId) -> Option<TextSize> {
        self.ast
            .nodes
            .get(node.index())
            .and_then(|node| node.source_range().start())
    }

    fn collect_imports(&self, declarations: &[AstNodeId]) -> Result<Vec<AstNodeId>, AstError> {
        let mut imports = Vec::new();
        for declaration in declarations {
            let node = self
                .ast
                .nodes
                .get(declaration.index())
                .ok_or(AstError::IndexOverflow)?;
            if node.kind() != GoAstKind::GenDecl {
                continue;
            }
            let fields = self
                .ast
                .fields
                .get(declaration.index())
                .ok_or(AstError::IndexOverflow)?;
            let is_import = fields.iter().any(|field| {
                field.field() == GoAstField::Tok
                    && field.value() == super::model::GoAstValue::Token(GoToken::Import)
            });
            if !is_import {
                continue;
            }
            let specs = fields
                .iter()
                .find_map(|field| (field.field() == GoAstField::Specs).then_some(field.value()));
            let Some(super::model::GoAstValue::Nodes(specs)) = specs else {
                return Err(AstError::InconsistentCst {
                    context: "GenDecl",
                    expected: "Specs field",
                });
            };
            imports.extend_from_slice(
                self.ast
                    .node_lists
                    .get(specs.start as usize..(specs.start + specs.count) as usize)
                    .ok_or(AstError::IndexOverflow)?,
            );
        }
        Ok(imports)
    }

    fn lower_comments(&mut self, tree: &Tree, top_end: TextSize) -> Result<(), AstError> {
        let mut cursor = tree.cursor(IterMode::INCLUDE_ANONYMOUS);
        let mut raw = Vec::new();
        loop {
            let node = cursor.node();
            if matches!(
                kind(&node),
                Some(GoKind::LineComment | GoKind::BlockComment)
            ) {
                raw.push(node);
            }
            if !cursor.next(true) {
                break;
            }
        }
        raw.sort_by_key(SyntaxNode::from);
        raw.dedup_by_key(|node| (node.from(), node.to()));

        let mut groups = Vec::<PendingCommentGroup>::new();
        for comment in raw {
            let text = self.node_text(&comment)?.replace('\r', "");
            if comment.to() <= top_end
                && kind(&comment) == Some(GoKind::LineComment)
                && let Some(version) = go_version(&text)
            {
                self.go_version = version;
            }
            let end = comment
                .from()
                .checked_add(TextSize::try_from(text.len()).map_err(|_| AstError::SourceTooLarge)?)
                .ok_or(AstError::SourceTooLarge)?;
            let id = self.ast.push_node(
                GoAstKind::Comment,
                GoSourceRange::new(Some(comment.from()), Some(end)),
            )?;
            self.ast
                .push_position_field(id, GoAstField::Slash, Some(comment.from()))?;
            self.ast.push_string_field(id, GoAstField::Text, &text)?;
            let lowered = LoweredComment {
                node: id,
                from: comment.from(),
                to: end,
            };
            let continues = groups.last().is_some_and(|group| {
                group.comments.last().is_some_and(|previous| {
                    self.comment_gap(previous.to, lowered.from)
                        .is_some_and(|line_breaks| line_breaks <= group.max_line_breaks)
                })
            });
            if continues {
                groups
                    .last_mut()
                    .expect("group exists")
                    .comments
                    .push(lowered);
            } else {
                groups.push(PendingCommentGroup {
                    comments: vec![lowered],
                    max_line_breaks: usize::from(self.comment_starts_line(lowered.from)),
                });
            }
        }

        for group in groups {
            let from = group
                .comments
                .first()
                .expect("comment group is not empty")
                .from;
            let to = group
                .comments
                .last()
                .expect("comment group is not empty")
                .to;
            let comments = group
                .comments
                .iter()
                .map(|comment| comment.node)
                .collect::<Vec<_>>();
            let node = self.ast.push_node(
                GoAstKind::CommentGroup,
                GoSourceRange::new(Some(from), Some(to)),
            )?;
            self.ast
                .push_nodes_field(node, GoAstField::List, &comments)?;
            self.comments.push(CommentGroup { node, from, to });
        }
        Ok(())
    }

    fn comment_gap(&self, from: TextSize, to: TextSize) -> Option<usize> {
        let gap = self.source_slice(TextRange::new(from, to)).ok()?;
        if !gap.chars().all(char::is_whitespace) {
            return None;
        }
        Some(line_break_count(gap))
    }

    fn leading_comment(&self, position: TextSize) -> Option<AstNodeId> {
        self.comments
            .iter()
            .rev()
            .find(|comment| {
                comment.to <= position
                    && self.comment_starts_line(comment.from)
                    && self
                        .comment_gap(comment.to, position)
                        .is_some_and(|line_breaks| line_breaks <= 1)
            })
            .map(|comment| comment.node)
    }

    fn comment_starts_line(&self, position: TextSize) -> bool {
        let prefix = &self.source[..usize::from(position)];
        let line_start = prefix
            .rfind(['\n', '\r'])
            .map_or(0, |index| index.saturating_add(1));
        prefix[line_start..].chars().all(char::is_whitespace)
    }

    fn trailing_comment(&self, position: TextSize) -> Option<AstNodeId> {
        self.comments
            .iter()
            .find(|comment| {
                comment.from >= position
                    && self
                        .comment_gap(position, comment.from)
                        .is_some_and(|line_breaks| line_breaks == 0)
            })
            .map(|comment| comment.node)
    }

    fn lower_declaration(&mut self, node: &GoDeclaration) -> Result<AstNodeId, AstError> {
        match node {
            GoDeclaration::Const(node) => self.lower_gen_decl(GoGeneralDeclaration::Const(node)),
            GoDeclaration::Type(node) => self.lower_gen_decl(GoGeneralDeclaration::Type(node)),
            GoDeclaration::Var(node) if node.keyword_token().is_some() => {
                self.lower_gen_decl(GoGeneralDeclaration::Var(node))
            }
            GoDeclaration::Function(node) => self.lower_function_declaration(node),
            GoDeclaration::Method(node) => self.lower_method_declaration(node),
            GoDeclaration::Var(_) => Err(AstError::InconsistentCst {
                context: "SourceFile",
                expected: "Go declaration",
            }),
        }
    }

    fn lower_gen_decl(&mut self, node: GoGeneralDeclaration<'_>) -> Result<AstNodeId, AstError> {
        let token = match node.kind() {
            GoGeneralDeclarationKind::Import => GoToken::Import,
            GoGeneralDeclarationKind::Const => GoToken::Const,
            GoGeneralDeclarationKind::Type => GoToken::Type,
            GoGeneralDeclarationKind::Var => GoToken::Var,
        };
        let keyword = node.keyword_token().ok_or(AstError::InconsistentCst {
            context: "general declaration",
            expected: "declaration keyword",
        })?;
        let spec_list = node.spec_list();
        let specs = if let Some(list) = &spec_list {
            list.specs()
                .map(|spec| self.lower_spec(&spec))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let spec = node.spec().ok_or(AstError::InconsistentCst {
                context: "general declaration",
                expected: "declaration spec",
            })?;
            vec![self.lower_spec(&spec)?]
        };
        let lparen = spec_list
            .as_ref()
            .and_then(GoSpecList::left_paren_token)
            .map(|token| token.from());
        let rparen = spec_list
            .as_ref()
            .and_then(GoSpecList::right_paren_token)
            .map(|token| token.from());
        let end = rparen
            .map(|position| position + TextSize::from(1))
            .or_else(|| specs.last().and_then(|spec| self.ast_node_end(*spec)))
            .unwrap_or(node.syntax().to());
        let declaration = self.ast.push_node(
            GoAstKind::GenDecl,
            GoSourceRange::new(Some(keyword.from()), Some(end)),
        )?;
        let doc = self.leading_comment(keyword.from());
        self.ast
            .push_node_field(declaration, GoAstField::Doc, doc)?;
        self.ast
            .push_position_field(declaration, GoAstField::TokPos, Some(keyword.from()))?;
        self.ast
            .push_token_field(declaration, GoAstField::Tok, token)?;
        self.ast
            .push_position_field(declaration, GoAstField::Lparen, lparen)?;
        self.ast
            .push_nodes_field(declaration, GoAstField::Specs, &specs)?;
        self.ast
            .push_position_field(declaration, GoAstField::Rparen, rparen)?;
        Ok(declaration)
    }

    fn lower_spec(&mut self, node: &GoSpec) -> Result<AstNodeId, AstError> {
        match node {
            GoSpec::Import(node) => self.lower_import_spec(node),
            GoSpec::Const(node) => self.lower_value_spec(GoValueSpec::Const(node)),
            GoSpec::Type(node) => self.lower_type_spec(node),
            GoSpec::Var(node) => self.lower_value_spec(GoValueSpec::Var(node)),
        }
    }

    fn lower_import_spec(&mut self, node: &GoImportSpec) -> Result<AstNodeId, AstError> {
        let path = node.path().ok_or(AstError::InconsistentCst {
            context: "ImportSpec",
            expected: "String",
        })?;
        let name = if let Some(name) = node.name() {
            Some(self.lower_ident(name.syntax())?)
        } else if let Some(dot) = node.dot_token() {
            Some(self.lower_synthetic_ident(&dot, ".")?)
        } else {
            None
        };
        let path = self.lower_basic_lit(path.syntax())?;
        let start = name
            .and_then(|name| {
                self.ast
                    .nodes
                    .get(name.index())
                    .and_then(|node| node.source_range().start())
            })
            .unwrap_or(node.syntax().from());
        let spec = self.ast.push_node(
            GoAstKind::ImportSpec,
            GoSourceRange::new(Some(start), Some(node.syntax().to())),
        )?;
        let doc = self.leading_comment(start);
        let comment = self.trailing_comment(node.syntax().to());
        self.ast.push_node_field(spec, GoAstField::Doc, doc)?;
        self.ast.push_node_field(spec, GoAstField::Name, name)?;
        self.ast
            .push_node_field(spec, GoAstField::Path, Some(path))?;
        self.ast
            .push_node_field(spec, GoAstField::Comment, comment)?;
        // ParseFile leaves EndPos unset; go/ast import merging is what synthesizes it.
        self.ast
            .push_position_field(spec, GoAstField::EndPos, None)?;
        Ok(spec)
    }

    fn lower_value_spec(&mut self, node: GoValueSpec<'_>) -> Result<AstNodeId, AstError> {
        let names = node
            .names()
            .iter()
            .map(|name| self.lower_ident(name.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let parts = node.parts();
        let ty = parts
            .ty()
            .map(|ty| self.lower_type(ty.syntax()))
            .transpose()?;
        let values = parts
            .values()
            .iter()
            .map(|value| self.lower_expr(value.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let syntax = node.syntax();
        let value = self.ast.push_node(
            GoAstKind::ValueSpec,
            GoSourceRange::new(Some(syntax.from()), Some(syntax.to())),
        )?;
        let doc = self.leading_comment(syntax.from());
        let comment = self.trailing_comment(syntax.to());
        self.ast.push_node_field(value, GoAstField::Doc, doc)?;
        self.ast
            .push_nodes_field(value, GoAstField::Names, &names)?;
        self.ast.push_node_field(value, GoAstField::Type, ty)?;
        self.ast
            .push_nodes_field(value, GoAstField::Values, &values)?;
        self.ast
            .push_node_field(value, GoAstField::Comment, comment)?;
        Ok(value)
    }

    fn lower_type_spec(&mut self, node: &GoTypeSpec) -> Result<AstNodeId, AstError> {
        let name = node.name().ok_or(AstError::InconsistentCst {
            context: "TypeSpec",
            expected: "DefName",
        })?;
        let name = self.lower_ident(name.syntax())?;
        let type_params = node
            .type_parameters()
            .map(|params| self.lower_field_list(params.syntax()))
            .transpose()?;
        let assign = node.assign_token().map(|token| token.from());
        let ty = node.ty().ok_or(AstError::InconsistentCst {
            context: "TypeSpec",
            expected: "type",
        })?;
        let ty = self.lower_type(ty.syntax())?;
        let syntax = node.syntax();
        let spec = self.ast.push_node(
            GoAstKind::TypeSpec,
            GoSourceRange::new(Some(syntax.from()), Some(syntax.to())),
        )?;
        let doc = self.leading_comment(syntax.from());
        let comment = self.trailing_comment(syntax.to());
        self.ast.push_node_field(spec, GoAstField::Doc, doc)?;
        self.ast
            .push_node_field(spec, GoAstField::Name, Some(name))?;
        self.ast
            .push_node_field(spec, GoAstField::TypeParams, type_params)?;
        self.ast
            .push_position_field(spec, GoAstField::Assign, assign)?;
        self.ast.push_node_field(spec, GoAstField::Type, Some(ty))?;
        self.ast
            .push_node_field(spec, GoAstField::Comment, comment)?;
        Ok(spec)
    }

    fn lower_function_declaration(&mut self, node: &GoFunctionDecl) -> Result<AstNodeId, AstError> {
        let name = node.name().ok_or(AstError::InconsistentCst {
            context: "FuncDecl",
            expected: "function name",
        })?;
        let body = node.body();
        self.lower_func_decl(
            GoSignature::FunctionDeclaration(node),
            name.syntax(),
            None,
            body,
        )
    }

    fn lower_method_declaration(&mut self, node: &GoMethodDecl) -> Result<AstNodeId, AstError> {
        let receiver = node.receiver().ok_or(AstError::InconsistentCst {
            context: "FuncDecl",
            expected: "method receiver",
        })?;
        let receiver = Some(self.lower_field_list(receiver.syntax())?);
        let name = node.name().ok_or(AstError::InconsistentCst {
            context: "FuncDecl",
            expected: "function name",
        })?;
        let body = node.body();
        self.lower_func_decl(
            GoSignature::MethodDeclaration(node),
            name.syntax(),
            receiver,
            body,
        )
    }

    fn lower_func_decl(
        &mut self,
        signature: GoSignature<'_>,
        name: &SyntaxNode,
        receiver: Option<AstNodeId>,
        body: Option<GoBlock>,
    ) -> Result<AstNodeId, AstError> {
        let func = signature.func_token().ok_or(AstError::InconsistentCst {
            context: "FuncDecl",
            expected: "func token",
        })?;
        let name = self.lower_ident(name)?;
        let function_type = self.lower_signature(&signature, Some(func.from()))?;
        let body = body
            .map(|body| self.lower_block(body.syntax()))
            .transpose()?;
        let end = body
            .and_then(|body| self.ast_node_end(body))
            .or_else(|| self.ast_node_end(function_type))
            .unwrap_or(signature.syntax().to());
        let declaration = self.ast.push_node(
            GoAstKind::FuncDecl,
            GoSourceRange::new(Some(func.from()), Some(end)),
        )?;
        let doc = self.leading_comment(func.from());
        self.ast
            .push_node_field(declaration, GoAstField::Doc, doc)?;
        self.ast
            .push_node_field(declaration, GoAstField::Recv, receiver)?;
        self.ast
            .push_node_field(declaration, GoAstField::Name, Some(name))?;
        self.ast
            .push_node_field(declaration, GoAstField::Type, Some(function_type))?;
        self.ast
            .push_node_field(declaration, GoAstField::Body, body)?;
        Ok(declaration)
    }

    fn lower_signature(
        &mut self,
        signature: &GoSignature<'_>,
        func: Option<TextSize>,
    ) -> Result<AstNodeId, AstError> {
        let params = signature.parameters().ok_or(AstError::InconsistentCst {
            context: "function signature",
            expected: "parameters",
        })?;
        let params = self.lower_field_list(params.syntax())?;
        let type_params = signature
            .type_parameters()
            .map(|params| self.lower_field_list(params.syntax()))
            .transpose()?;
        let results = signature
            .result_parameters()
            .map(|results| self.lower_field_list(results.syntax()))
            .transpose()?
            .or(signature
                .result_type()
                .map(|ty| self.lower_single_result(ty.syntax()))
                .transpose()?);
        let start = func.unwrap_or_else(|| {
            self.ast
                .nodes
                .get(params.index())
                .and_then(|node| node.source_range().start())
                .unwrap_or(signature.syntax().from())
        });
        let end = results
            .and_then(|results| self.ast_node_end(results))
            .or_else(|| self.ast_node_end(params))
            .unwrap_or(signature.syntax().to());
        let function = self.ast.push_node(
            GoAstKind::FuncType,
            GoSourceRange::new(Some(start), Some(end)),
        )?;
        self.ast
            .push_position_field(function, GoAstField::Func, func)?;
        self.ast
            .push_node_field(function, GoAstField::TypeParams, type_params)?;
        self.ast
            .push_node_field(function, GoAstField::Params, Some(params))?;
        self.ast
            .push_node_field(function, GoAstField::Results, results)?;
        Ok(function)
    }

    fn lower_single_result(&mut self, ty: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let field = self.lower_unnamed_field(ty)?;
        let list = self.ast.push_node(
            GoAstKind::FieldList,
            GoSourceRange::new(Some(ty.from()), Some(ty.to())),
        )?;
        self.ast
            .push_position_field(list, GoAstField::Opening, None)?;
        self.ast
            .push_nodes_field(list, GoAstField::List, &[field])?;
        self.ast
            .push_position_field(list, GoAstField::Closing, None)?;
        Ok(list)
    }

    fn lower_ident(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let text = self.node_text(node)?.to_owned();
        self.lower_synthetic_ident(node, &text)
    }

    fn lower_synthetic_ident(
        &mut self,
        node: &SyntaxNode,
        name: &str,
    ) -> Result<AstNodeId, AstError> {
        let ident = self.ast.push_node(GoAstKind::Ident, node.range().into())?;
        self.ast
            .push_position_field(ident, GoAstField::NamePos, Some(node.from()))?;
        self.ast.push_string_field(ident, GoAstField::Name, name)?;
        Ok(ident)
    }

    fn lower_basic_lit(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let source = self.node_text(node)?;
        let kind = match kind(node) {
            Some(GoKind::Rune) => GoToken::Char,
            Some(GoKind::String) => GoToken::String,
            Some(GoKind::Number) => number_token(source),
            _ => {
                return Err(AstError::InconsistentCst {
                    context: "literal",
                    expected: "Number, Rune, or String",
                });
            }
        };
        let value = if source.starts_with('`') {
            source.replace('\r', "")
        } else {
            source.to_owned()
        };
        let literal = self
            .ast
            .push_node(GoAstKind::BasicLit, node.range().into())?;
        self.ast
            .push_position_field(literal, GoAstField::ValuePos, Some(node.from()))?;
        self.ast
            .push_position_field(literal, GoAstField::ValueEnd, Some(node.to()))?;
        self.ast.push_token_field(literal, GoAstField::Kind, kind)?;
        self.ast
            .push_string_field(literal, GoAstField::Value, &value)?;
        Ok(literal)
    }

    fn lower_type(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        match kind(node) {
            Some(GoKind::TypeName) => self.lower_ident(node),
            Some(GoKind::QualifiedType) => self.lower_selector(node),
            Some(GoKind::ParameterizedType) => self.lower_parameterized(node),
            Some(GoKind::PointerType) => self.lower_star(node),
            Some(GoKind::FunctionType) => {
                let function = GoFunctionType::downcast_from(node.clone()).map_err(|_| {
                    AstError::InconsistentCst {
                        context: "function type",
                        expected: "FunctionType",
                    }
                })?;
                let signature = GoSignature::FunctionType(&function);
                let func = signature.func_token().map(|token| token.from());
                self.lower_signature(&signature, func)
            }
            Some(GoKind::InterfaceType) => self.lower_interface_type(node),
            Some(GoKind::ChannelType) => self.lower_channel_type(node),
            Some(GoKind::ParenthesizedType) => self.lower_paren(node),
            Some(GoKind::StructType) => self.lower_struct_type(node),
            Some(GoKind::ArrayType | GoKind::SliceType) => self.lower_array_type(node),
            Some(GoKind::MapType) => self.lower_map_type(node),
            _ => Err(AstError::InconsistentCst {
                context: "type",
                expected: "Go type",
            }),
        }
    }

    fn lower_expr(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let mut results = Vec::new();
        let mut actions = vec![GoExpressionAction::Visit(node.clone())];
        while let Some(action) = actions.pop() {
            match action {
                GoExpressionAction::Visit(node) => {
                    let shape =
                        expression_shape(&node).map_err(|error| AstError::InconsistentCst {
                            context: error.context(),
                            expected: error.expected(),
                        })?;
                    match shape {
                        GoExpressionShape::Atom(atom) => {
                            results.push(self.lower_expr_atom(&atom)?);
                        }
                        GoExpressionShape::Unary { operator, operand } => {
                            actions.push(GoExpressionAction::LowerUnary(operator));
                            actions.push(GoExpressionAction::Visit(operand));
                        }
                        GoExpressionShape::Binary {
                            left,
                            operator,
                            right,
                        } => {
                            actions.push(GoExpressionAction::LowerBinary(operator));
                            actions.push(GoExpressionAction::Visit(right));
                            actions.push(GoExpressionAction::Visit(left));
                        }
                        GoExpressionShape::Postfix { base, expression } => {
                            actions.push(GoExpressionAction::LowerPostfix(expression));
                            actions.push(GoExpressionAction::Visit(base));
                        }
                    }
                }
                GoExpressionAction::LowerUnary(operator) => {
                    let operand = results.pop().ok_or(AstError::InconsistentCst {
                        context: "unary expression",
                        expected: "lowered operand",
                    })?;
                    results.push(self.lower_unary_fields(&operator, operand)?);
                }
                GoExpressionAction::LowerBinary(operator) => {
                    let right = results.pop().ok_or(AstError::InconsistentCst {
                        context: "binary expression",
                        expected: "lowered right operand",
                    })?;
                    let left = results.pop().ok_or(AstError::InconsistentCst {
                        context: "binary expression",
                        expected: "lowered left operand",
                    })?;
                    let token = binary_token_for(self.node_text(&operator)?).ok_or(
                        AstError::InconsistentCst {
                            context: "binary expression",
                            expected: "Go binary operator",
                        },
                    )?;
                    results.push(self.lower_binary_operator(&operator, token, left, right)?);
                }
                GoExpressionAction::LowerPostfix(expression) => {
                    let base = results.pop().ok_or(AstError::InconsistentCst {
                        context: "postfix expression",
                        expected: "lowered base expression",
                    })?;
                    results.push(self.lower_postfix_layer(&expression, base)?);
                }
            }
        }
        if results.len() != 1 {
            return Err(AstError::InconsistentCst {
                context: "expression",
                expected: "one complete Go expression",
            });
        }
        Ok(results
            .pop()
            .expect("one complete Go expression has one result"))
    }

    fn lower_expr_atom(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        match kind(node) {
            Some(GoKind::Number | GoKind::Rune | GoKind::String) => self.lower_basic_lit(node),
            Some(
                GoKind::Bool | GoKind::Nil | GoKind::VariableName | GoKind::Make | GoKind::New,
            ) => self.lower_ident(node),
            Some(GoKind::TypedLiteral | GoKind::LiteralValue) => self.lower_composite_lit(node),
            Some(GoKind::ParenthesizedExpr | GoKind::ParenthesizedType) => self.lower_paren(node),
            Some(GoKind::FunctionLiteral) => self.lower_func_lit(node),
            Some(GoKind::Conversion) => self.lower_conversion(node),
            _ if crate::GoType::downcast_from(node.clone()).is_ok() => self.lower_type(node),
            _ => Err(AstError::InconsistentCst {
                context: "expression atom",
                expected: "Go primary operand",
            }),
        }
    }

    fn lower_postfix_layer(
        &mut self,
        node: &SyntaxNode,
        base: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        match kind(node) {
            Some(GoKind::SelectorExpr) => self.lower_selector_fields(node, base),
            Some(GoKind::IndexExpr | GoKind::IndexListExpr) => {
                self.lower_index_with_base(node, kind(node) == Some(GoKind::IndexListExpr), base)
            }
            Some(GoKind::ParameterizedExpr) => self.lower_parameterized_with_base(node, base),
            Some(GoKind::SliceExpr) => self.lower_slice_with_base(node, base),
            Some(GoKind::TypeAssertion) => self.lower_type_assertion_with_base(node, base),
            Some(GoKind::CallExpr) => self.lower_call_with_function(node, base),
            _ => Err(AstError::InconsistentCst {
                context: "postfix expression",
                expected: "Go postfix expression",
            }),
        }
    }

    fn lower_paren(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let (expression, left, right) = match kind(node) {
            Some(GoKind::ParenthesizedExpr) => {
                let parenthesized = required(
                    GoParenthesizedExpr::downcast_from(node.clone()).ok(),
                    "parenthesized expression",
                    "ParenthesizedExpr",
                )?;
                let expression = required(
                    parenthesized.expression(),
                    "parenthesized expression",
                    "expression",
                )?;
                let left = required(
                    parenthesized.left_paren_token(),
                    "parenthesized expression",
                    "left parenthesis",
                )?;
                let right = required(
                    parenthesized.right_paren_token(),
                    "parenthesized expression",
                    "right parenthesis",
                )?;
                (expression.into_syntax(), left, right)
            }
            Some(GoKind::ParenthesizedType) => {
                let parenthesized = required(
                    GoParenthesizedType::downcast_from(node.clone()).ok(),
                    "parenthesized type",
                    "ParenthesizedType",
                )?;
                let ty = required(parenthesized.ty(), "parenthesized type", "type")?;
                let left = required(
                    parenthesized.left_paren_token(),
                    "parenthesized type",
                    "left parenthesis",
                )?;
                let right = required(
                    parenthesized.right_paren_token(),
                    "parenthesized type",
                    "right parenthesis",
                )?;
                (ty.into_syntax(), left, right)
            }
            _ => {
                return Err(AstError::InconsistentCst {
                    context: "parenthesized expression",
                    expected: "parenthesized expression or type",
                });
            }
        };
        let expression = self.lower_expr(&expression)?;
        let paren = self
            .ast
            .push_node(GoAstKind::ParenExpr, node.range().into())?;
        self.ast
            .push_position_field(paren, GoAstField::Lparen, Some(left.from()))?;
        self.ast
            .push_node_field(paren, GoAstField::X, Some(expression))?;
        self.ast
            .push_position_field(paren, GoAstField::Rparen, Some(right.from()))?;
        Ok(paren)
    }

    fn lower_selector(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        if kind(node) == Some(GoKind::SelectorExpr) {
            let selector = required(
                GoSelectorExpr::downcast_from(node.clone()).ok(),
                "selector",
                "SelectorExpr",
            )?;
            let qualifier = required(selector.qualifier(), "selector", "qualifier expression")?;
            let qualifier = self.lower_expr(qualifier.syntax())?;
            return self.lower_selector_fields(node, qualifier);
        }

        let qualified_type = required(
            GoQualifiedType::downcast_from(node.clone()).ok(),
            "qualified type",
            "QualifiedType",
        )?;
        let mut qualifiers = qualified_type.qualifiers();
        let first = required(qualifiers.next(), "qualified type", "qualifier")?;
        let mut qualifier = self.lower_ident(first.syntax())?;
        for name in qualifiers {
            let selected = self.lower_ident(name.syntax())?;
            let from = self.ast_node_start(qualifier).unwrap_or(node.from());
            let to = self.ast_node_end(selected).unwrap_or(name.syntax().to());
            let selector = self.ast.push_node(
                GoAstKind::SelectorExpr,
                GoSourceRange::new(Some(from), Some(to)),
            )?;
            self.ast
                .push_node_field(selector, GoAstField::X, Some(qualifier))?;
            self.ast
                .push_node_field(selector, GoAstField::Sel, Some(selected))?;
            qualifier = selector;
        }
        let selected = required(
            qualified_type.name(),
            "qualified type",
            "selected type name",
        )?;
        let selected = self.lower_ident(selected.syntax())?;
        let range = TextRange::new(
            self.ast
                .nodes
                .get(qualifier.index())
                .and_then(|node| node.source_range().start())
                .unwrap_or(node.from()),
            self.ast_node_end(selected).unwrap_or(node.to()),
        );
        let selector = self.ast.push_node(GoAstKind::SelectorExpr, range.into())?;
        self.ast
            .push_node_field(selector, GoAstField::X, Some(qualifier))?;
        self.ast
            .push_node_field(selector, GoAstField::Sel, Some(selected))?;
        Ok(selector)
    }

    fn lower_selector_fields(
        &mut self,
        node: &SyntaxNode,
        qualifier: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let selector = required(
            GoSelectorExpr::downcast_from(node.clone()).ok(),
            "selector",
            "SelectorExpr",
        )?;
        let selected = required(selector.name(), "selector", "selected field")?;
        let selected = self.lower_ident(selected.syntax())?;
        let start = self.ast_node_start(qualifier).unwrap_or(node.from());
        let end = self.ast_node_end(selected).unwrap_or(node.to());
        let selector = self.ast.push_node(
            GoAstKind::SelectorExpr,
            GoSourceRange::new(Some(start), Some(end)),
        )?;
        self.ast
            .push_node_field(selector, GoAstField::X, Some(qualifier))?;
        self.ast
            .push_node_field(selector, GoAstField::Sel, Some(selected))?;
        Ok(selector)
    }

    fn lower_parameterized(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let base = match kind(node) {
            Some(GoKind::ParameterizedExpr) => {
                let parameterized = required(
                    GoParameterizedExpr::downcast_from(node.clone()).ok(),
                    "parameterized expression",
                    "ParameterizedExpr",
                )?;
                required(
                    parameterized.base(),
                    "parameterized expression",
                    "base expression",
                )?
                .into_syntax()
            }
            Some(GoKind::ParameterizedType) => {
                let parameterized = required(
                    GoParameterizedType::downcast_from(node.clone()).ok(),
                    "parameterized type",
                    "ParameterizedType",
                )?;
                required(parameterized.base(), "parameterized type", "base type")?.into_syntax()
            }
            _ => {
                return Err(AstError::InconsistentCst {
                    context: "parameterized expression",
                    expected: "parameterized expression or type",
                });
            }
        };
        let base = self.lower_expr(&base)?;
        self.lower_parameterized_with_base(node, base)
    }

    fn lower_parameterized_with_base(
        &mut self,
        node: &SyntaxNode,
        base: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let args = match kind(node) {
            Some(GoKind::ParameterizedExpr) => {
                let parameterized = required(
                    GoParameterizedExpr::downcast_from(node.clone()).ok(),
                    "parameterized expression",
                    "ParameterizedExpr",
                )?;
                required(
                    parameterized.arguments(),
                    "parameterized expression",
                    "TypeArgs",
                )?
            }
            Some(GoKind::ParameterizedType) => {
                let parameterized = required(
                    GoParameterizedType::downcast_from(node.clone()).ok(),
                    "parameterized type",
                    "ParameterizedType",
                )?;
                required(parameterized.arguments(), "parameterized type", "TypeArgs")?
            }
            _ => {
                return Err(AstError::InconsistentCst {
                    context: "parameterized expression",
                    expected: "parameterized expression or type",
                });
            }
        };
        let indices = args
            .types()
            .map(|ty| self.lower_expr(ty.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let left = required(args.left_bracket_token(), "TypeArgs", "left bracket")?;
        let right = required(args.right_bracket_token(), "TypeArgs", "right bracket")?;
        let start = self.ast_node_start(base).unwrap_or(node.from());
        self.lower_index_fields(
            TextRange::new(start, node.to()),
            left.from(),
            right.from(),
            base,
            &indices,
        )
    }

    fn lower_index_with_base(
        &mut self,
        node: &SyntaxNode,
        list: bool,
        base: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let shape = index_shape(node).map_err(|error| AstError::InconsistentCst {
            context: error.context(),
            expected: error.expected(),
        })?;
        let indices = shape
            .indices()
            .iter()
            .map(|index| self.lower_expr(index.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        if list != shape.is_list() || list != (indices.len() > 1) {
            return Err(AstError::InconsistentCst {
                context: "index expression",
                expected: "matching index cardinality",
            });
        }
        let start = self.ast_node_start(base).unwrap_or(node.from());
        self.lower_index_fields(
            TextRange::new(start, node.to()),
            shape.left_bracket().from(),
            shape.right_bracket().from(),
            base,
            &indices,
        )
    }

    fn lower_index_fields(
        &mut self,
        range: TextRange,
        left: TextSize,
        right: TextSize,
        base: AstNodeId,
        indices: &[AstNodeId],
    ) -> Result<AstNodeId, AstError> {
        if indices.is_empty() {
            return Err(AstError::InconsistentCst {
                context: "index expression",
                expected: "at least one index",
            });
        }
        let index = self.ast.push_node(
            if indices.len() == 1 {
                GoAstKind::IndexExpr
            } else {
                GoAstKind::IndexListExpr
            },
            range.into(),
        )?;
        self.ast.push_node_field(index, GoAstField::X, Some(base))?;
        self.ast
            .push_position_field(index, GoAstField::Lbrack, Some(left))?;
        if indices.len() == 1 {
            self.ast
                .push_node_field(index, GoAstField::Index, indices.first().copied())?;
        } else {
            self.ast
                .push_nodes_field(index, GoAstField::Indices, indices)?;
        }
        self.ast
            .push_position_field(index, GoAstField::Rbrack, Some(right))?;
        Ok(index)
    }

    fn lower_star(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let pointer = required(
            GoPointerType::downcast_from(node.clone()).ok(),
            "pointer type",
            "PointerType",
        )?;
        let star = required(pointer.star_token(), "pointer type", "asterisk")?;
        let operand = required(pointer.ty(), "pointer type", "operand type")?;
        let operand = self.lower_expr(operand.syntax())?;
        let expression = self
            .ast
            .push_node(GoAstKind::StarExpr, node.range().into())?;
        self.ast
            .push_position_field(expression, GoAstField::Star, Some(star.from()))?;
        self.ast
            .push_node_field(expression, GoAstField::X, Some(operand))?;
        Ok(expression)
    }

    fn lower_array_type(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let (left, element, length) = match kind(node) {
            Some(GoKind::ArrayType) => {
                let array = required(
                    GoArrayType::downcast_from(node.clone()).ok(),
                    "array type",
                    "ArrayType",
                )?;
                let left = required(array.left_bracket_token(), "array type", "left bracket")?;
                let element = required(array.ty(), "array type", "element type")?;
                let length = required(array.length(), "array type", "length expression")?;
                let length = Some(self.lower_expr(length.syntax())?);
                (left, element, length)
            }
            Some(GoKind::SliceType) => {
                let slice = required(
                    GoSliceType::downcast_from(node.clone()).ok(),
                    "slice type",
                    "SliceType",
                )?;
                let left = required(slice.left_bracket_token(), "slice type", "left bracket")?;
                let element = required(slice.ty(), "slice type", "element type")?;
                let length = if let Some(ellipsis) = slice.ellipsis_token() {
                    let length = self
                        .ast
                        .push_node(GoAstKind::Ellipsis, ellipsis.range().into())?;
                    self.ast.push_position_field(
                        length,
                        GoAstField::Ellipsis,
                        Some(ellipsis.from()),
                    )?;
                    self.ast.push_node_field(length, GoAstField::Elt, None)?;
                    Some(length)
                } else {
                    None
                };
                (left, element, length)
            }
            _ => {
                return Err(AstError::InconsistentCst {
                    context: "array type",
                    expected: "ArrayType or SliceType",
                });
            }
        };
        let ty = self.lower_type(element.syntax())?;
        let array = self
            .ast
            .push_node(GoAstKind::ArrayType, node.range().into())?;
        self.ast
            .push_position_field(array, GoAstField::Lbrack, Some(left.from()))?;
        self.ast.push_node_field(array, GoAstField::Len, length)?;
        self.ast.push_node_field(array, GoAstField::Elt, Some(ty))?;
        Ok(array)
    }

    fn lower_struct_type(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let structure = required(
            GoStructType::downcast_from(node.clone()).ok(),
            "struct type",
            "StructType",
        )?;
        let keyword = required(structure.struct_token(), "struct type", "struct token")?;
        let body = required(structure.body(), "struct type", "StructBody")?;
        let fields = self.lower_field_list(body.syntax())?;
        let structure = self
            .ast
            .push_node(GoAstKind::StructType, node.range().into())?;
        self.ast
            .push_position_field(structure, GoAstField::Struct, Some(keyword.from()))?;
        self.ast
            .push_node_field(structure, GoAstField::Fields, Some(fields))?;
        self.ast
            .push_bool_field(structure, GoAstField::Incomplete, false)?;
        Ok(structure)
    }

    fn lower_interface_type(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let interface = required(
            GoInterfaceType::downcast_from(node.clone()).ok(),
            "interface type",
            "InterfaceType",
        )?;
        let keyword = required(
            interface.interface_token(),
            "interface type",
            "interface token",
        )?;
        let body = required(interface.body(), "interface type", "InterfaceBody")?;
        let methods = self.lower_field_list(body.syntax())?;
        let interface = self
            .ast
            .push_node(GoAstKind::InterfaceType, node.range().into())?;
        self.ast
            .push_position_field(interface, GoAstField::Interface, Some(keyword.from()))?;
        self.ast
            .push_node_field(interface, GoAstField::Methods, Some(methods))?;
        self.ast
            .push_bool_field(interface, GoAstField::Incomplete, false)?;
        Ok(interface)
    }

    fn lower_map_type(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let map_type = required(
            GoMapType::downcast_from(node.clone()).ok(),
            "map type",
            "MapType",
        )?;
        let keyword = required(map_type.map_token(), "map type", "map token")?;
        let key = required(map_type.key(), "map type", "key type")?;
        let value = required(map_type.value(), "map type", "value type")?;
        let key = self.lower_type(key.syntax())?;
        let value = self.lower_type(value.syntax())?;
        let map = self
            .ast
            .push_node(GoAstKind::MapType, node.range().into())?;
        self.ast
            .push_position_field(map, GoAstField::Map, Some(keyword.from()))?;
        self.ast.push_node_field(map, GoAstField::Key, Some(key))?;
        self.ast
            .push_node_field(map, GoAstField::Value, Some(value))?;
        Ok(map)
    }

    fn lower_channel_type(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let channel_type = required(
            GoChannelType::downcast_from(node.clone()).ok(),
            "channel type",
            "ChannelType",
        )?;
        let shape =
            channel_type_shape(&channel_type).map_err(|error| AstError::InconsistentCst {
                context: error.context(),
                expected: error.expected(),
            })?;
        self.lower_channel_type_shape(&shape)
    }

    fn lower_channel_type_shape(
        &mut self,
        shape: &GoChannelTypeShape,
    ) -> Result<AstNodeId, AstError> {
        let mut value = self.lower_type(shape.value().syntax())?;
        for layer in shape.layers().iter().rev() {
            value = self.lower_channel_type_layer(layer, value)?;
        }
        Ok(value)
    }

    fn lower_channel_type_layer(
        &mut self,
        layer: &GoChannelTypeLayer,
        value: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let direction = match layer.direction() {
            GoChannelTypeDirection::SendReceive => GoChanDirection::SendReceive,
            GoChannelTypeDirection::Send => GoChanDirection::Send,
            GoChannelTypeDirection::Receive => GoChanDirection::Receive,
        };
        self.lower_channel_type_fields(
            layer.range(),
            layer.begin(),
            layer.arrow().map(SyntaxNode::from),
            direction,
            value,
        )
    }

    fn lower_channel_type_fields(
        &mut self,
        range: TextRange,
        begin: TextSize,
        arrow: Option<TextSize>,
        direction: GoChanDirection,
        value: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let channel = self.ast.push_node(GoAstKind::ChanType, range.into())?;
        self.ast
            .push_position_field(channel, GoAstField::Begin, Some(begin))?;
        self.ast
            .push_position_field(channel, GoAstField::Arrow, arrow)?;
        self.ast
            .push_direction_field(channel, GoAstField::Dir, direction)?;
        self.ast
            .push_node_field(channel, GoAstField::Value, Some(value))?;
        Ok(channel)
    }

    fn lower_func_lit(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let function = GoFunctionLiteral::downcast_from(node.clone()).map_err(|_| {
            AstError::InconsistentCst {
                context: "function literal",
                expected: "FunctionLiteral",
            }
        })?;
        let signature = GoSignature::FunctionLiteral(&function);
        let func = signature.func_token().ok_or(AstError::InconsistentCst {
            context: "function literal",
            expected: "func token",
        })?;
        let ty = self.lower_signature(&signature, Some(func.from()))?;
        let body = function.body().ok_or(AstError::InconsistentCst {
            context: "function literal",
            expected: "Block",
        })?;
        let body = self.lower_block(body.syntax())?;
        let literal = self
            .ast
            .push_node(GoAstKind::FuncLit, node.range().into())?;
        self.ast
            .push_node_field(literal, GoAstField::Type, Some(ty))?;
        self.ast
            .push_node_field(literal, GoAstField::Body, Some(body))?;
        Ok(literal)
    }

    fn lower_conversion(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let conversion = required(
            GoConversion::downcast_from(node.clone()).ok(),
            "conversion",
            "Conversion",
        )?;
        let shape = conversion_shape(&conversion).map_err(|error| AstError::InconsistentCst {
            context: error.context(),
            expected: error.expected(),
        })?;
        let fun = match shape.function() {
            GoConversionFunction::Type(ty) => self.lower_type(ty.syntax())?,
            GoConversionFunction::Channel(channel) => self.lower_channel_type_shape(channel)?,
        };
        let argument = self.lower_expr(shape.argument().syntax())?;
        let call_start = self.ast_node_start(fun).unwrap_or(node.from());
        let mut expression = self.lower_call_fields(
            TextRange::new(call_start, node.to()),
            fun,
            shape.left_paren().from(),
            &[argument],
            None,
            shape.right_paren().from(),
        )?;
        for prefix in shape.prefixes().iter().rev() {
            expression = self.lower_unary_fields(prefix, expression)?;
        }
        Ok(expression)
    }

    fn lower_call(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        self.lower_expr(node)
    }

    fn lower_call_with_function(
        &mut self,
        node: &SyntaxNode,
        function: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let call = required(
            GoCallExpr::downcast_from(node.clone()).ok(),
            "call expression",
            "CallExpr",
        )?;
        let arguments = required(call.arguments(), "call expression", "Arguments")?;
        let args = arguments
            .values()
            .map(|value| self.lower_expr(value.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let left = required(
            arguments.left_paren_token(),
            "Arguments",
            "left parenthesis",
        )?;
        let right = required(
            arguments.right_paren_token(),
            "Arguments",
            "right parenthesis",
        )?;
        let ellipsis = arguments.ellipsis_token().map(|token| token.from());
        let start = self.ast_node_start(function).unwrap_or(node.from());
        self.lower_call_fields(
            TextRange::new(start, node.to()),
            function,
            left.from(),
            &args,
            ellipsis,
            right.from(),
        )
    }

    fn lower_call_fields(
        &mut self,
        range: TextRange,
        function: AstNodeId,
        left: TextSize,
        args: &[AstNodeId],
        ellipsis: Option<TextSize>,
        right: TextSize,
    ) -> Result<AstNodeId, AstError> {
        let call = self.ast.push_node(GoAstKind::CallExpr, range.into())?;
        self.ast
            .push_node_field(call, GoAstField::Fun, Some(function))?;
        self.ast
            .push_position_field(call, GoAstField::Lparen, Some(left))?;
        self.ast.push_nodes_field(call, GoAstField::Args, args)?;
        self.ast
            .push_position_field(call, GoAstField::Ellipsis, ellipsis)?;
        self.ast
            .push_position_field(call, GoAstField::Rparen, Some(right))?;
        Ok(call)
    }

    fn lower_unary_fields(
        &mut self,
        operator: &SyntaxNode,
        operand: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let token = token_for(self.node_text(operator)?).ok_or(AstError::InconsistentCst {
            context: "unary expression",
            expected: "Go unary operator",
        })?;
        let end = self.ast_node_end(operand);
        let range = GoSourceRange::new(Some(operator.from()), end);
        if token == GoToken::Mul {
            let expression = self.ast.push_node(GoAstKind::StarExpr, range)?;
            self.ast
                .push_position_field(expression, GoAstField::Star, Some(operator.from()))?;
            self.ast
                .push_node_field(expression, GoAstField::X, Some(operand))?;
            return Ok(expression);
        }
        let expression = self.ast.push_node(GoAstKind::UnaryExpr, range)?;
        self.ast
            .push_position_field(expression, GoAstField::OpPos, Some(operator.from()))?;
        self.ast
            .push_token_field(expression, GoAstField::Op, token)?;
        self.ast
            .push_node_field(expression, GoAstField::X, Some(operand))?;
        Ok(expression)
    }

    fn lower_binary_operator(
        &mut self,
        operator: &SyntaxNode,
        token: GoToken,
        left: AstNodeId,
        right: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let start = self.ast_node_start(left).unwrap_or(operator.from());
        let end = self.ast_node_end(right).unwrap_or(operator.to());
        let expression = self.ast.push_node(
            GoAstKind::BinaryExpr,
            GoSourceRange::new(Some(start), Some(end)),
        )?;
        self.ast
            .push_node_field(expression, GoAstField::X, Some(left))?;
        self.ast
            .push_position_field(expression, GoAstField::OpPos, Some(operator.from()))?;
        self.ast
            .push_token_field(expression, GoAstField::Op, token)?;
        self.ast
            .push_node_field(expression, GoAstField::Y, Some(right))?;
        Ok(expression)
    }

    fn lower_composite_lit(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let (ty, body) = if kind(node) == Some(GoKind::TypedLiteral) {
            let literal = required(
                GoTypedLiteral::downcast_from(node.clone()).ok(),
                "typed literal",
                "TypedLiteral",
            )?;
            let ty = required(literal.ty(), "typed literal", "type")?;
            let body = required(literal.body(), "typed literal", "LiteralValue")?;
            (Some(self.lower_type(ty.syntax())?), body)
        } else {
            let body = required(
                crate::GoLiteralValue::downcast_from(node.clone()).ok(),
                "composite literal",
                "LiteralValue",
            )?;
            (None, body)
        };
        let left = required(body.left_brace_token(), "composite literal", "left brace")?;
        let right = required(body.right_brace_token(), "composite literal", "right brace")?;
        let elements = body
            .elements()
            .map(|element| self.lower_element(element.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let literal = self
            .ast
            .push_node(GoAstKind::CompositeLit, node.range().into())?;
        self.ast.push_node_field(literal, GoAstField::Type, ty)?;
        self.ast
            .push_position_field(literal, GoAstField::Lbrace, Some(left.from()))?;
        self.ast
            .push_nodes_field(literal, GoAstField::Elts, &elements)?;
        self.ast
            .push_position_field(literal, GoAstField::Rbrace, Some(right.from()))?;
        self.ast
            .push_bool_field(literal, GoAstField::Incomplete, false)?;
        Ok(literal)
    }

    fn lower_element(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let element = required(
            GoElement::downcast_from(node.clone()).ok(),
            "composite element",
            "Element",
        )?;
        let value = required(element.value(), "composite element", "value")?;
        let value = self.lower_expr(value.syntax())?;
        let Some(key) = element.key() else {
            return Ok(value);
        };
        let key_value = required(key.value(), "composite key", "key expression")?;
        let colon = required(element.colon_token(), "composite key", "colon")?;
        let key_value = self.lower_expr(key_value.syntax())?;
        let expression = self
            .ast
            .push_node(GoAstKind::KeyValueExpr, node.range().into())?;
        self.ast
            .push_node_field(expression, GoAstField::Key, Some(key_value))?;
        self.ast
            .push_position_field(expression, GoAstField::Colon, Some(colon.from()))?;
        self.ast
            .push_node_field(expression, GoAstField::Value, Some(value))?;
        Ok(expression)
    }

    fn lower_slice_with_base(
        &mut self,
        node: &SyntaxNode,
        base: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let shape = slice_shape(node).map_err(|error| AstError::InconsistentCst {
            context: error.context(),
            expected: error.expected(),
        })?;
        let low = shape
            .low()
            .map(|expression| self.lower_expr(expression.syntax()))
            .transpose()?;
        let high = shape
            .high()
            .map(|expression| self.lower_expr(expression.syntax()))
            .transpose()?;
        let max = shape
            .max()
            .map(|expression| self.lower_expr(expression.syntax()))
            .transpose()?;
        let start = self.ast_node_start(base).unwrap_or(node.from());
        let expression = self.ast.push_node(
            GoAstKind::SliceExpr,
            GoSourceRange::new(Some(start), Some(node.to())),
        )?;
        self.ast
            .push_node_field(expression, GoAstField::X, Some(base))?;
        self.ast.push_position_field(
            expression,
            GoAstField::Lbrack,
            Some(shape.left_bracket().from()),
        )?;
        self.ast.push_node_field(expression, GoAstField::Low, low)?;
        self.ast
            .push_node_field(expression, GoAstField::High, high)?;
        self.ast.push_node_field(expression, GoAstField::Max, max)?;
        self.ast
            .push_bool_field(expression, GoAstField::Slice3, shape.is_full())?;
        self.ast.push_position_field(
            expression,
            GoAstField::Rbrack,
            Some(shape.right_bracket().from()),
        )?;
        Ok(expression)
    }

    fn lower_type_assertion_with_base(
        &mut self,
        node: &SyntaxNode,
        expression: AstNodeId,
    ) -> Result<AstNodeId, AstError> {
        let assertion = required(
            GoTypeAssertion::downcast_from(node.clone()).ok(),
            "type assertion",
            "TypeAssertion",
        )?;
        let asserted = required(assertion.ty(), "type assertion", "type")?;
        let left = required(
            assertion.left_paren_token(),
            "type assertion",
            "left parenthesis",
        )?;
        let right = required(
            assertion.right_paren_token(),
            "type assertion",
            "right parenthesis",
        )?;
        let asserted = Some(self.lower_type(asserted.syntax())?);
        let start = self.ast_node_start(expression).unwrap_or(node.from());
        self.lower_type_assertion_fields(
            TextRange::new(start, node.to()),
            expression,
            left.from(),
            asserted,
            right.from(),
        )
    }

    fn lower_type_assertion_fields(
        &mut self,
        range: TextRange,
        expression: AstNodeId,
        left: TextSize,
        asserted: Option<AstNodeId>,
        right: TextSize,
    ) -> Result<AstNodeId, AstError> {
        let assertion = self
            .ast
            .push_node(GoAstKind::TypeAssertExpr, range.into())?;
        self.ast
            .push_node_field(assertion, GoAstField::X, Some(expression))?;
        self.ast
            .push_position_field(assertion, GoAstField::Lparen, Some(left))?;
        self.ast
            .push_node_field(assertion, GoAstField::Type, asserted)?;
        self.ast
            .push_position_field(assertion, GoAstField::Rparen, Some(right))?;
        Ok(assertion)
    }

    fn lower_field_list(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let (opening, closing, fields) = match kind(node) {
            Some(GoKind::Parameters) => {
                let parameters = required(
                    GoParameters::downcast_from(node.clone()).ok(),
                    "field list",
                    "Parameters",
                )?;
                let opening = parameters.left_paren_token().map(|token| token.from());
                let closing = parameters.right_paren_token().map(|token| token.from());
                let fields = parameters
                    .fields()
                    .map(TypedNode::into_syntax)
                    .collect::<Vec<_>>();
                (opening, closing, fields)
            }
            Some(GoKind::TypeParams) => {
                let parameters = required(
                    GoTypeParams::downcast_from(node.clone()).ok(),
                    "field list",
                    "TypeParams",
                )?;
                let opening = parameters.left_bracket_token().map(|token| token.from());
                let closing = parameters.right_bracket_token().map(|token| token.from());
                let fields = parameters
                    .fields()
                    .map(TypedNode::into_syntax)
                    .collect::<Vec<_>>();
                (opening, closing, fields)
            }
            Some(GoKind::StructBody) => {
                let body = required(
                    GoStructBody::downcast_from(node.clone()).ok(),
                    "field list",
                    "StructBody",
                )?;
                let opening = body.left_brace_token().map(|token| token.from());
                let closing = body.right_brace_token().map(|token| token.from());
                let fields = body
                    .fields()
                    .map(TypedNode::into_syntax)
                    .collect::<Vec<_>>();
                (opening, closing, fields)
            }
            Some(GoKind::InterfaceBody) => {
                let body = required(
                    GoInterfaceBody::downcast_from(node.clone()).ok(),
                    "field list",
                    "InterfaceBody",
                )?;
                let opening = body.left_brace_token().map(|token| token.from());
                let closing = body.right_brace_token().map(|token| token.from());
                let fields = body
                    .fields()
                    .map(TypedNode::into_syntax)
                    .collect::<Vec<_>>();
                (opening, closing, fields)
            }
            _ => {
                return Err(AstError::InconsistentCst {
                    context: "field list",
                    expected: "parameters or type body",
                });
            }
        };
        let fields = fields
            .iter()
            .map(|field| self.lower_field(field))
            .collect::<Result<Vec<_>, _>>()?;
        let list = self
            .ast
            .push_node(GoAstKind::FieldList, node.range().into())?;
        self.ast
            .push_position_field(list, GoAstField::Opening, opening)?;
        self.ast.push_nodes_field(list, GoAstField::List, &fields)?;
        self.ast
            .push_position_field(list, GoAstField::Closing, closing)?;
        Ok(list)
    }

    fn lower_field(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        match kind(node) {
            Some(GoKind::Parameter) => self.lower_parameter_field(node),
            Some(GoKind::TypeParam) => self.lower_type_parameter_field(node),
            Some(GoKind::FieldDecl) => self.lower_struct_field(node),
            Some(GoKind::MethodElem) => self.lower_method_field(node),
            Some(GoKind::TypeElem | GoKind::UnderlyingType) => self.lower_interface_field(node),
            _ if crate::GoType::downcast_from(node.clone()).is_ok() => {
                self.lower_interface_field(node)
            }
            _ => Err(AstError::InconsistentCst {
                context: "field list",
                expected: "field",
            }),
        }
    }

    fn lower_parameter_field(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let parameter = required(
            GoParameter::downcast_from(node.clone()).ok(),
            "parameter",
            "Parameter",
        )?;
        let names = parameter
            .names()
            .map(|name| self.lower_ident(name.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = required(parameter.ty(), "parameter", "type")?;
        let mut ty = self.lower_type(ty.syntax())?;
        if let Some(ellipsis) = parameter.ellipsis_token() {
            let variadic = self.ast.push_node(
                GoAstKind::Ellipsis,
                GoSourceRange::new(Some(ellipsis.from()), self.ast_node_end(ty)),
            )?;
            self.ast
                .push_position_field(variadic, GoAstField::Ellipsis, Some(ellipsis.from()))?;
            self.ast
                .push_node_field(variadic, GoAstField::Elt, Some(ty))?;
            ty = variadic;
        }
        self.lower_field_values(node.range(), &names, ty, None, FieldComments::default())
    }

    fn lower_type_parameter_field(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let parameter = required(
            GoTypeParam::downcast_from(node.clone()).ok(),
            "type parameter",
            "TypeParam",
        )?;
        let shape =
            type_parameter_shape(&parameter).map_err(|error| AstError::InconsistentCst {
                context: error.context(),
                expected: error.expected(),
            })?;
        let names = shape
            .names()
            .iter()
            .map(|name| self.lower_ident(name.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let constraint = self.lower_type_element(shape.constraint())?;
        self.lower_field_values(
            node.range(),
            &names,
            constraint,
            None,
            FieldComments::default(),
        )
    }

    fn lower_struct_field(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let field = required(
            GoFieldDecl::downcast_from(node.clone()).ok(),
            "struct field",
            "FieldDecl",
        )?;
        let names = field
            .names()
            .map(|name| self.lower_ident(name.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let type_node = required(field.ty(), "struct field", "type")?;
        let mut ty = self.lower_type(type_node.syntax())?;
        if names.is_empty()
            && let Some(star) = field.star_token()
        {
            let pointer = self.ast.push_node(
                GoAstKind::StarExpr,
                GoSourceRange::new(Some(star.from()), self.ast_node_end(ty)),
            )?;
            self.ast
                .push_position_field(pointer, GoAstField::Star, Some(star.from()))?;
            self.ast.push_node_field(pointer, GoAstField::X, Some(ty))?;
            ty = pointer;
        }
        let tag = field
            .tag()
            .map(|tag| self.lower_basic_lit(tag.syntax()))
            .transpose()?;
        let comments = self.field_comments(node.range());
        self.lower_field_values(node.range(), &names, ty, tag, comments)
    }

    fn lower_method_field(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let method =
            GoMethodElem::downcast_from(node.clone()).map_err(|_| AstError::InconsistentCst {
                context: "interface method",
                expected: "MethodElem",
            })?;
        let name = method.name().ok_or(AstError::InconsistentCst {
            context: "interface method",
            expected: "method name",
        })?;
        let name = self.lower_ident(name.syntax())?;
        let ty = self.lower_signature(&GoSignature::MethodElement(&method), None)?;
        let comments = self.field_comments(node.range());
        self.lower_field_values(node.range(), &[name], ty, None, comments)
    }

    fn lower_interface_field(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let ty = self.lower_type_element(node)?;
        let comments = self.field_comments(node.range());
        self.lower_field_values(node.range(), &[], ty, None, comments)
    }

    fn lower_unnamed_field(&mut self, ty: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let ty_id = self.lower_type(ty)?;
        self.lower_field_values(ty.range(), &[], ty_id, None, FieldComments::default())
    }

    fn field_comments(&self, range: TextRange) -> FieldComments {
        FieldComments {
            doc: self.leading_comment(range.start()),
            line: self.trailing_comment(range.end()),
        }
    }

    fn lower_field_values(
        &mut self,
        range: TextRange,
        names: &[AstNodeId],
        ty: AstNodeId,
        tag: Option<AstNodeId>,
        comments: FieldComments,
    ) -> Result<AstNodeId, AstError> {
        let field = self.ast.push_node(GoAstKind::Field, range.into())?;
        self.ast
            .push_node_field(field, GoAstField::Doc, comments.doc)?;
        self.ast.push_nodes_field(field, GoAstField::Names, names)?;
        self.ast
            .push_node_field(field, GoAstField::Type, Some(ty))?;
        self.ast.push_node_field(field, GoAstField::Tag, tag)?;
        self.ast
            .push_node_field(field, GoAstField::Comment, comments.line)?;
        Ok(field)
    }

    fn lower_type_element(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        match kind(node) {
            Some(GoKind::UnderlyingType) => {
                let underlying = required(
                    GoUnderlyingType::downcast_from(node.clone()).ok(),
                    "underlying type",
                    "UnderlyingType",
                )?;
                let tilde = required(underlying.tilde_token(), "underlying type", "tilde")?;
                let ty = required(underlying.ty(), "underlying type", "type")?;
                let ty = self.lower_type(ty.syntax())?;
                let expression = self
                    .ast
                    .push_node(GoAstKind::UnaryExpr, node.range().into())?;
                self.ast
                    .push_position_field(expression, GoAstField::OpPos, Some(tilde.from()))?;
                self.ast
                    .push_token_field(expression, GoAstField::Op, GoToken::Tilde)?;
                self.ast
                    .push_node_field(expression, GoAstField::X, Some(ty))?;
                Ok(expression)
            }
            Some(GoKind::TypeElem) => {
                let element = required(
                    GoTypeElem::downcast_from(node.clone()).ok(),
                    "type element",
                    "TypeElem",
                )?;
                let mut operands = element.terms();
                let first = operands.next().ok_or(AstError::InconsistentCst {
                    context: "type element",
                    expected: "operand",
                })?;
                let mut left = self.lower_type_element(first.syntax())?;
                for (operator, right) in element.operators().zip(operands) {
                    let right = self.lower_type_element(right.syntax())?;
                    let from = self
                        .ast
                        .nodes
                        .get(left.index())
                        .and_then(|node| node.source_range().start())
                        .unwrap_or(node.from());
                    let to = self.ast_node_end(right).unwrap_or(node.to());
                    let binary = self.ast.push_node(
                        GoAstKind::BinaryExpr,
                        GoSourceRange::new(Some(from), Some(to)),
                    )?;
                    self.ast
                        .push_node_field(binary, GoAstField::X, Some(left))?;
                    self.ast.push_position_field(
                        binary,
                        GoAstField::OpPos,
                        Some(operator.syntax().from()),
                    )?;
                    self.ast
                        .push_token_field(binary, GoAstField::Op, GoToken::Or)?;
                    self.ast
                        .push_node_field(binary, GoAstField::Y, Some(right))?;
                    left = binary;
                }
                Ok(left)
            }
            _ if crate::GoType::downcast_from(node.clone()).is_ok() => self.lower_type(node),
            _ => Err(AstError::InconsistentCst {
                context: "type element",
                expected: "type term",
            }),
        }
    }

    fn lower_block(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let (left, right) = match kind(node) {
            Some(GoKind::Block) => {
                let block = required(GoBlock::downcast_from(node.clone()).ok(), "block", "Block")?;
                (block.left_brace_token(), block.right_brace_token())
            }
            Some(GoKind::SwitchBlock) => {
                let block = required(
                    GoSwitchBlock::downcast_from(node.clone()).ok(),
                    "block",
                    "SwitchBlock",
                )?;
                (block.left_brace_token(), block.right_brace_token())
            }
            Some(GoKind::SelectBlock) => {
                let block = required(
                    GoSelectBlock::downcast_from(node.clone()).ok(),
                    "block",
                    "SelectBlock",
                )?;
                (block.left_brace_token(), block.right_brace_token())
            }
            _ => {
                return Err(AstError::InconsistentCst {
                    context: "block",
                    expected: "Block, SwitchBlock, or SelectBlock",
                });
            }
        };
        let left = required(left, "block", "left brace")?;
        let right = required(right, "block", "right brace")?;
        let statements = self.lower_statement_list(node)?;
        let block = self
            .ast
            .push_node(GoAstKind::BlockStmt, node.range().into())?;
        self.ast
            .push_position_field(block, GoAstField::Lbrace, Some(left.from()))?;
        self.ast
            .push_nodes_field(block, GoAstField::List, &statements)?;
        self.ast
            .push_position_field(block, GoAstField::Rbrace, Some(right.from()))?;
        Ok(block)
    }

    fn lower_statement_list(&mut self, node: &SyntaxNode) -> Result<Vec<AstNodeId>, AstError> {
        match statement_list(node) {
            GoStatementListShape::Plain(items) => self.lower_statement_children(&items),
            GoStatementListShape::Switch(clauses) => clauses
                .iter()
                .map(|clause| self.lower_case_clause_shape(clause))
                .collect(),
            GoStatementListShape::Select(clauses) => clauses
                .iter()
                .map(|clause| self.lower_comm_clause_shape(clause))
                .collect(),
        }
    }

    fn lower_statement_children(
        &mut self,
        children: &[GoStatementListItem],
    ) -> Result<Vec<AstNodeId>, AstError> {
        let mut statements = Vec::new();
        let mut previous_statement_end = None;
        let mut index = 0;
        while index < children.len() {
            let child = &children[index];
            if let GoStatementListItem::Semicolon(token) = child {
                let terminates_previous = previous_statement_end.is_some_and(|end| {
                    self.source_slice(TextRange::new(end, token.from()))
                        .is_ok_and(|gap| line_break_count(gap) == 0)
                });
                if !terminates_previous {
                    statements.push(self.lower_empty_statement(token, false)?);
                }
                previous_statement_end = None;
                index += 1;
                continue;
            }

            let GoStatementListItem::Statement(statement) = child else {
                index += 1;
                continue;
            };
            let syntax = statement.syntax();
            let (statement, consumes_following) = if kind(syntax) == Some(GoKind::LabeledStatement)
            {
                self.lower_labeled_statement_followed_by(syntax, children.get(index + 1))?
            } else {
                (self.lower_statement(syntax)?, false)
            };
            statements.push(statement);
            if consumes_following {
                previous_statement_end = None;
                index += 2;
            } else {
                previous_statement_end = Some(syntax.to());
                index += 1;
            }
        }
        Ok(statements)
    }

    fn lower_case_clause_shape(&mut self, clause: &GoClauseShape) -> Result<AstNodeId, AstError> {
        self.lower_case_clause(clause.header().syntax(), clause.body())
    }

    fn lower_comm_clause_shape(&mut self, clause: &GoClauseShape) -> Result<AstNodeId, AstError> {
        self.lower_comm_clause(clause.header().syntax(), clause.body())
    }

    fn lower_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        match kind(node) {
            Some(GoKind::ExprStatement) => self.lower_expr_statement(node),
            Some(GoKind::SendStatement) => self.lower_send_statement(node),
            Some(GoKind::IncDecStatement) => self.lower_inc_dec_statement(node),
            Some(GoKind::Assignment) => self.lower_assignment(node),
            Some(GoKind::VarDecl) => {
                let declaration = GoVarDecl::downcast_from(node.clone()).map_err(|_| {
                    AstError::InconsistentCst {
                        context: "declaration statement",
                        expected: "VarDecl",
                    }
                })?;
                if declaration.keyword_token().is_none() {
                    return self.lower_assignment(node);
                }
                let declaration = self.lower_gen_decl(GoGeneralDeclaration::Var(&declaration))?;
                let statement = self
                    .ast
                    .push_node(GoAstKind::DeclStmt, node.range().into())?;
                self.ast
                    .push_node_field(statement, GoAstField::Decl, Some(declaration))?;
                Ok(statement)
            }
            Some(GoKind::ConstDecl) => {
                let declaration = GoConstDecl::downcast_from(node.clone()).map_err(|_| {
                    AstError::InconsistentCst {
                        context: "declaration statement",
                        expected: "ConstDecl",
                    }
                })?;
                let declaration = self.lower_gen_decl(GoGeneralDeclaration::Const(&declaration))?;
                let statement = self
                    .ast
                    .push_node(GoAstKind::DeclStmt, node.range().into())?;
                self.ast
                    .push_node_field(statement, GoAstField::Decl, Some(declaration))?;
                Ok(statement)
            }
            Some(GoKind::TypeDecl) => {
                let declaration = GoTypeDecl::downcast_from(node.clone()).map_err(|_| {
                    AstError::InconsistentCst {
                        context: "declaration statement",
                        expected: "TypeDecl",
                    }
                })?;
                let declaration = self.lower_gen_decl(GoGeneralDeclaration::Type(&declaration))?;
                let statement = self
                    .ast
                    .push_node(GoAstKind::DeclStmt, node.range().into())?;
                self.ast
                    .push_node_field(statement, GoAstField::Decl, Some(declaration))?;
                Ok(statement)
            }
            Some(GoKind::Block) => self.lower_block(node),
            Some(GoKind::LabeledStatement) => self.lower_labeled_statement(node),
            Some(GoKind::IfStatement) => self.lower_if_statement(node),
            Some(GoKind::SwitchStatement) => self.lower_switch_statement(node),
            Some(GoKind::TypeSwitchStatement) => self.lower_type_switch_statement(node),
            Some(GoKind::ForStatement) => self.lower_for_statement(node),
            Some(GoKind::GoStatement) => self.lower_go_statement(node, false),
            Some(GoKind::SelectStatement) => self.lower_select_statement(node),
            Some(GoKind::ReturnStatement) => self.lower_return_statement(node),
            Some(GoKind::GotoStatement) => self.lower_branch_statement(node),
            Some(GoKind::FallthroughStatement) => self.lower_fallthrough_statement(node),
            Some(GoKind::DeferStatement) => self.lower_go_statement(node, true),
            Some(GoKind::ReceiveStatement) => self.lower_receive_statement(node),
            _ => Err(AstError::InconsistentCst {
                context: "statement",
                expected: "Go statement",
            }),
        }
    }

    fn lower_expr_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let statement = required(
            GoExprStatement::downcast_from(node.clone()).ok(),
            "expression statement",
            "ExprStatement",
        )?;
        let expression = required(statement.expression(), "expression statement", "expression")?;
        let expression = self.lower_expr(expression.syntax())?;
        let statement = self
            .ast
            .push_node(GoAstKind::ExprStmt, node.range().into())?;
        self.ast
            .push_node_field(statement, GoAstField::X, Some(expression))?;
        Ok(statement)
    }

    fn lower_send_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let send = required(
            GoSendStatement::downcast_from(node.clone()).ok(),
            "send statement",
            "SendStatement",
        )?;
        let channel = required(send.channel(), "send statement", "channel expression")?;
        let value = required(send.value(), "send statement", "value expression")?;
        let arrow = required(send.arrow_token(), "send statement", "channel arrow")?;
        let channel = self.lower_expr(channel.syntax())?;
        let value = self.lower_expr(value.syntax())?;
        let statement = self
            .ast
            .push_node(GoAstKind::SendStmt, node.range().into())?;
        self.ast
            .push_node_field(statement, GoAstField::Chan, Some(channel))?;
        self.ast
            .push_position_field(statement, GoAstField::Arrow, Some(arrow.from()))?;
        self.ast
            .push_node_field(statement, GoAstField::Value, Some(value))?;
        Ok(statement)
    }

    fn lower_inc_dec_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let update = required(
            GoIncDecStatement::downcast_from(node.clone()).ok(),
            "increment/decrement statement",
            "IncDecStatement",
        )?;
        let expression = required(
            update.expression(),
            "increment/decrement statement",
            "expression",
        )?;
        let operator = required(
            update.operator(),
            "increment/decrement statement",
            "operator",
        )?;
        let token =
            token_for(self.node_text(operator.syntax())?).ok_or(AstError::InconsistentCst {
                context: "increment/decrement statement",
                expected: "++ or --",
            })?;
        let expression = self.lower_expr(expression.syntax())?;
        let statement = self
            .ast
            .push_node(GoAstKind::IncDecStmt, node.range().into())?;
        self.ast
            .push_node_field(statement, GoAstField::X, Some(expression))?;
        self.ast.push_position_field(
            statement,
            GoAstField::TokPos,
            Some(operator.syntax().from()),
        )?;
        self.ast
            .push_token_field(statement, GoAstField::Tok, token)?;
        Ok(statement)
    }

    fn lower_assignment(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let shape = assignment_shape(node);
        let operator = required(shape.operator(), "assignment", "assignment operator")?;
        let token = token_for(self.node_text(operator)?).ok_or(AstError::InconsistentCst {
            context: "assignment",
            expected: "Go assignment operator",
        })?;
        let lhs = shape
            .left()
            .iter()
            .map(|operand| {
                if operand.is_definition() {
                    self.lower_ident(operand.syntax())
                } else {
                    self.lower_expr(operand.syntax())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rhs = shape
            .right()
            .iter()
            .map(|expression| self.lower_expr(expression.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        if lhs.is_empty() || rhs.is_empty() {
            return Err(AstError::InconsistentCst {
                context: "assignment",
                expected: "left and right expressions",
            });
        }
        let statement = self
            .ast
            .push_node(GoAstKind::AssignStmt, node.range().into())?;
        self.ast
            .push_nodes_field(statement, GoAstField::Lhs, &lhs)?;
        self.ast
            .push_position_field(statement, GoAstField::TokPos, Some(operator.from()))?;
        self.ast
            .push_token_field(statement, GoAstField::Tok, token)?;
        self.ast
            .push_nodes_field(statement, GoAstField::Rhs, &rhs)?;
        Ok(statement)
    }

    fn lower_receive_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let shape = assignment_shape(node);
        if shape.operator().is_some() {
            self.lower_assignment(node)
        } else {
            let expression = required(
                shape.right().first(),
                "receive statement",
                "receive expression",
            )?;
            let expression = self.lower_expr(expression.syntax())?;
            let statement = self
                .ast
                .push_node(GoAstKind::ExprStmt, node.range().into())?;
            self.ast
                .push_node_field(statement, GoAstField::X, Some(expression))?;
            Ok(statement)
        }
    }

    fn lower_labeled_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let (statement, _) = self.lower_labeled_statement_followed_by(node, None)?;
        Ok(statement)
    }

    fn lower_labeled_statement_followed_by(
        &mut self,
        node: &SyntaxNode,
        following: Option<&GoStatementListItem>,
    ) -> Result<(AstNodeId, bool), AstError> {
        let labeled = required(
            GoLabeledStatement::downcast_from(node.clone()).ok(),
            "labeled statement",
            "LabeledStatement",
        )?;
        let label = required(labeled.label(), "labeled statement", "label")?;
        let colon = required(labeled.colon_token(), "labeled statement", "colon")?;
        let label = self.lower_ident(label.syntax())?;
        let (body, consumes_following) = if let Some(body) = labeled.body() {
            (self.lower_statement(body.syntax())?, false)
        } else {
            match following {
                Some(GoStatementListItem::Semicolon(empty)) => {
                    (self.lower_empty_statement(empty, false)?, true)
                }
                Some(GoStatementListItem::RightBrace(right)) => {
                    (self.lower_implicit_empty(right.from())?, false)
                }
                _ => (self.lower_implicit_empty(colon.to())?, false),
            }
        };
        let end = self.ast_node_end(body).unwrap_or(node.to());
        let statement = self.ast.push_node(
            GoAstKind::LabeledStmt,
            GoSourceRange::new(Some(node.from()), Some(end)),
        )?;
        self.ast
            .push_node_field(statement, GoAstField::Label, Some(label))?;
        self.ast
            .push_position_field(statement, GoAstField::Colon, Some(colon.from()))?;
        self.ast
            .push_node_field(statement, GoAstField::Stmt, Some(body))?;
        Ok((statement, consumes_following))
    }

    fn lower_if_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let if_statement = required(
            GoIfStatement::downcast_from(node.clone()).ok(),
            "if statement",
            "IfStatement",
        )?;
        let keyword = required(if_statement.if_token(), "if statement", "if token")?;
        let condition = required(if_statement.condition(), "if statement", "condition")?;
        let body = required(if_statement.body(), "if statement", "then block")?;
        let init = if_statement
            .init()
            .map(|init| self.lower_statement(init.syntax()))
            .transpose()?;
        let condition = self.lower_expr(condition.syntax())?;
        let body_id = self.lower_block(body.syntax())?;
        let alternate = if let Some(alternate) = if_statement.alternate_if() {
            Some(self.lower_statement(alternate.syntax())?)
        } else if let Some(alternate) = if_statement.alternate_block() {
            Some(self.lower_statement(alternate.syntax())?)
        } else {
            None
        };
        let end = alternate
            .and_then(|alternate| self.ast_node_end(alternate))
            .or_else(|| self.ast_node_end(body_id))
            .unwrap_or(node.to());
        let statement = self.ast.push_node(
            GoAstKind::IfStmt,
            GoSourceRange::new(Some(keyword.from()), Some(end)),
        )?;
        self.ast
            .push_position_field(statement, GoAstField::If, Some(keyword.from()))?;
        self.ast
            .push_node_field(statement, GoAstField::Init, init)?;
        self.ast
            .push_node_field(statement, GoAstField::Cond, Some(condition))?;
        self.ast
            .push_node_field(statement, GoAstField::Body, Some(body_id))?;
        self.ast
            .push_node_field(statement, GoAstField::Else, alternate)?;
        Ok(statement)
    }

    fn lower_switch_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let switch = required(
            GoSwitchStatement::downcast_from(node.clone()).ok(),
            "switch statement",
            "SwitchStatement",
        )?;
        let keyword = required(switch.switch_token(), "switch statement", "switch token")?;
        let body = required(switch.body(), "switch statement", "SwitchBlock")?;
        let init = switch
            .init()
            .map(|init| self.lower_statement(init.syntax()))
            .transpose()?;
        let tag = switch
            .tag()
            .map(|tag| self.lower_expr(tag.syntax()))
            .transpose()?;
        let body = self.lower_block(body.syntax())?;
        let statement = self
            .ast
            .push_node(GoAstKind::SwitchStmt, node.range().into())?;
        self.ast
            .push_position_field(statement, GoAstField::Switch, Some(keyword.from()))?;
        self.ast
            .push_node_field(statement, GoAstField::Init, init)?;
        self.ast.push_node_field(statement, GoAstField::Tag, tag)?;
        self.ast
            .push_node_field(statement, GoAstField::Body, Some(body))?;
        Ok(statement)
    }

    fn lower_type_switch_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let type_switch = required(
            GoTypeSwitchStatement::downcast_from(node.clone()).ok(),
            "type switch",
            "TypeSwitchStatement",
        )?;
        let keyword = required(type_switch.switch_token(), "type switch", "switch token")?;
        let body = required(type_switch.body(), "type switch", "SwitchBlock")?;
        let init = type_switch
            .init()
            .map(|init| self.lower_statement(init.syntax()))
            .transpose()?;
        let expression = required(
            type_switch.expression(),
            "type switch",
            "asserted expression",
        )?;
        let left = required(
            type_switch.left_paren_token(),
            "type switch",
            "left parenthesis",
        )?;
        let right = required(
            type_switch.right_paren_token(),
            "type switch",
            "right parenthesis",
        )?;
        let expression_id = self.lower_expr(expression.syntax())?;
        let assertion = self.lower_type_assertion_fields(
            TextRange::new(expression.syntax().from(), right.to()),
            expression_id,
            left.from(),
            None,
            right.from(),
        )?;
        let assign = if let Some(name) = type_switch.name() {
            let name_from = name.syntax().from();
            let name = self.lower_ident(name.syntax())?;
            let define = required(type_switch.define_token(), "type switch", "define token")?;
            let assignment = self.ast.push_node(
                GoAstKind::AssignStmt,
                GoSourceRange::new(Some(name_from), self.ast_node_end(assertion)),
            )?;
            self.ast
                .push_nodes_field(assignment, GoAstField::Lhs, &[name])?;
            self.ast
                .push_position_field(assignment, GoAstField::TokPos, Some(define.from()))?;
            self.ast
                .push_token_field(assignment, GoAstField::Tok, GoToken::Define)?;
            self.ast
                .push_nodes_field(assignment, GoAstField::Rhs, &[assertion])?;
            assignment
        } else {
            let statement = self.ast.push_node(
                GoAstKind::ExprStmt,
                GoSourceRange::new(
                    Some(expression.syntax().from()),
                    self.ast_node_end(assertion),
                ),
            )?;
            self.ast
                .push_node_field(statement, GoAstField::X, Some(assertion))?;
            statement
        };
        let body = self.lower_block(body.syntax())?;
        let statement = self
            .ast
            .push_node(GoAstKind::TypeSwitchStmt, node.range().into())?;
        self.ast
            .push_position_field(statement, GoAstField::Switch, Some(keyword.from()))?;
        self.ast
            .push_node_field(statement, GoAstField::Init, init)?;
        self.ast
            .push_node_field(statement, GoAstField::Assign, Some(assign))?;
        self.ast
            .push_node_field(statement, GoAstField::Body, Some(body))?;
        Ok(statement)
    }

    fn lower_for_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let for_statement = required(
            GoForStatement::downcast_from(node.clone()).ok(),
            "for statement",
            "ForStatement",
        )?;
        let keyword = required(for_statement.for_token(), "for statement", "for token")?;
        let body = required(for_statement.body(), "for statement", "Block")?;
        if let Some(range) = for_statement.range_clause() {
            return self.lower_range_statement(node, &keyword, range.syntax(), body.syntax());
        }
        let clause = if let Some(clause) = for_statement.clause() {
            self.lower_for_clause(clause.syntax())?
        } else {
            let condition = for_statement
                .condition()
                .map(|condition| self.lower_expr(condition.syntax()))
                .transpose()?;
            LoweredForClause {
                init: None,
                condition,
                post: None,
            }
        };
        let body = self.lower_block(body.syntax())?;
        let statement = self
            .ast
            .push_node(GoAstKind::ForStmt, node.range().into())?;
        self.ast
            .push_position_field(statement, GoAstField::For, Some(keyword.from()))?;
        self.ast
            .push_node_field(statement, GoAstField::Init, clause.init)?;
        self.ast
            .push_node_field(statement, GoAstField::Cond, clause.condition)?;
        self.ast
            .push_node_field(statement, GoAstField::Post, clause.post)?;
        self.ast
            .push_node_field(statement, GoAstField::Body, Some(body))?;
        Ok(statement)
    }

    fn lower_for_clause(&mut self, node: &SyntaxNode) -> Result<LoweredForClause, AstError> {
        let clause = required(
            GoForClause::downcast_from(node.clone()).ok(),
            "for clause",
            "ForClause",
        )?;
        let shape = for_clause_shape(&clause).map_err(|error| AstError::InconsistentCst {
            context: error.context(),
            expected: error.expected(),
        })?;
        let init = shape
            .init()
            .map(|init| self.lower_statement(init.syntax()))
            .transpose()?;
        let condition = shape
            .condition()
            .map(|condition| self.lower_expr(condition.syntax()))
            .transpose()?;
        let post = shape
            .post()
            .map(|post| self.lower_statement(post.syntax()))
            .transpose()?;
        Ok(LoweredForClause {
            init,
            condition,
            post,
        })
    }

    fn lower_range_statement(
        &mut self,
        node: &SyntaxNode,
        keyword: &SyntaxNode,
        clause: &SyntaxNode,
        body: &SyntaxNode,
    ) -> Result<AstNodeId, AstError> {
        let shape = range_shape(clause).map_err(|error| AstError::InconsistentCst {
            context: error.context(),
            expected: error.expected(),
        })?;
        let lhs = shape
            .left()
            .iter()
            .map(|operand| {
                if matches!(operand, GoAssignmentOperand::Definition(_)) {
                    self.lower_ident(operand.syntax())
                } else {
                    self.lower_expr(operand.syntax())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ranged = self.lower_expr(shape.expression().syntax())?;
        let body = self.lower_block(body)?;
        let statement = self
            .ast
            .push_node(GoAstKind::RangeStmt, node.range().into())?;
        self.ast
            .push_position_field(statement, GoAstField::For, Some(keyword.from()))?;
        self.ast
            .push_node_field(statement, GoAstField::Key, lhs.first().copied())?;
        self.ast
            .push_node_field(statement, GoAstField::Value, lhs.get(1).copied())?;
        self.ast.push_position_field(
            statement,
            GoAstField::TokPos,
            shape.operator().map(SyntaxNode::from),
        )?;
        let token = shape
            .operator()
            .and_then(|operator| self.node_text(operator).ok())
            .and_then(token_for)
            .unwrap_or(GoToken::Illegal);
        self.ast
            .push_token_field(statement, GoAstField::Tok, token)?;
        self.ast.push_position_field(
            statement,
            GoAstField::Range,
            Some(shape.range_token().from()),
        )?;
        self.ast
            .push_node_field(statement, GoAstField::X, Some(ranged))?;
        self.ast
            .push_node_field(statement, GoAstField::Body, Some(body))?;
        Ok(statement)
    }

    fn lower_go_statement(
        &mut self,
        node: &SyntaxNode,
        defer: bool,
    ) -> Result<AstNodeId, AstError> {
        let (keyword, expression) = if defer {
            let statement = required(
                GoDeferStatement::downcast_from(node.clone()).ok(),
                "defer statement",
                "DeferStatement",
            )?;
            let keyword = required(statement.defer_token(), "defer statement", "defer token")?;
            let expression = required(statement.expression(), "defer statement", "expression")?;
            (keyword, expression)
        } else {
            let statement = required(
                GoGoStatement::downcast_from(node.clone()).ok(),
                "go statement",
                "GoStatement",
            )?;
            let keyword = required(statement.go_token(), "go statement", "go token")?;
            let expression = required(statement.expression(), "go statement", "expression")?;
            (keyword, expression)
        };
        if kind(expression.syntax()) != Some(GoKind::CallExpr) {
            return Err(AstError::InconsistentCst {
                context: "go/defer statement",
                expected: "CallExpr",
            });
        }
        let call = self.lower_call(expression.syntax())?;
        let statement = self.ast.push_node(
            if defer {
                GoAstKind::DeferStmt
            } else {
                GoAstKind::GoStmt
            },
            node.range().into(),
        )?;
        self.ast.push_position_field(
            statement,
            if defer {
                GoAstField::Defer
            } else {
                GoAstField::Go
            },
            Some(keyword.from()),
        )?;
        self.ast
            .push_node_field(statement, GoAstField::Call, Some(call))?;
        Ok(statement)
    }

    fn lower_return_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let return_statement = required(
            GoReturnStatement::downcast_from(node.clone()).ok(),
            "return statement",
            "ReturnStatement",
        )?;
        let keyword = required(
            return_statement.return_token(),
            "return statement",
            "return token",
        )?;
        let results = return_statement
            .results()
            .map(|result| self.lower_expr(result.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let statement = self
            .ast
            .push_node(GoAstKind::ReturnStmt, node.range().into())?;
        self.ast
            .push_position_field(statement, GoAstField::Return, Some(keyword.from()))?;
        self.ast
            .push_nodes_field(statement, GoAstField::Results, &results)?;
        Ok(statement)
    }

    fn lower_branch_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let branch = required(
            GoGotoStatement::downcast_from(node.clone()).ok(),
            "branch statement",
            "GotoStatement",
        )?;
        let (keyword, token) = if let Some(keyword) = branch.break_token() {
            (keyword, GoToken::Break)
        } else if let Some(keyword) = branch.continue_token() {
            (keyword, GoToken::Continue)
        } else if let Some(keyword) = branch.goto_token() {
            (keyword, GoToken::Goto)
        } else {
            return Err(AstError::InconsistentCst {
                context: "branch statement",
                expected: "branch keyword",
            });
        };
        let label = branch
            .label()
            .map(|label| self.lower_ident(label.syntax()))
            .transpose()?;
        let statement = self
            .ast
            .push_node(GoAstKind::BranchStmt, node.range().into())?;
        self.ast
            .push_position_field(statement, GoAstField::TokPos, Some(keyword.from()))?;
        self.ast
            .push_token_field(statement, GoAstField::Tok, token)?;
        self.ast
            .push_node_field(statement, GoAstField::Label, label)?;
        Ok(statement)
    }

    fn lower_fallthrough_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let fallthrough = required(
            GoFallthroughStatement::downcast_from(node.clone()).ok(),
            "fallthrough statement",
            "FallthroughStatement",
        )?;
        let keyword = required(
            fallthrough.fallthrough_token(),
            "fallthrough statement",
            "fallthrough token",
        )?;
        let statement = self
            .ast
            .push_node(GoAstKind::BranchStmt, node.range().into())?;
        self.ast
            .push_position_field(statement, GoAstField::TokPos, Some(keyword.from()))?;
        self.ast
            .push_token_field(statement, GoAstField::Tok, GoToken::Fallthrough)?;
        self.ast
            .push_node_field(statement, GoAstField::Label, None)?;
        Ok(statement)
    }

    fn lower_select_statement(&mut self, node: &SyntaxNode) -> Result<AstNodeId, AstError> {
        let select = required(
            GoSelectStatement::downcast_from(node.clone()).ok(),
            "select statement",
            "SelectStatement",
        )?;
        let keyword = required(select.select_token(), "select statement", "select token")?;
        let body = required(select.body(), "select statement", "SelectBlock")?;
        let body = self.lower_block(body.syntax())?;
        let statement = self
            .ast
            .push_node(GoAstKind::SelectStmt, node.range().into())?;
        self.ast
            .push_position_field(statement, GoAstField::Select, Some(keyword.from()))?;
        self.ast
            .push_node_field(statement, GoAstField::Body, Some(body))?;
        Ok(statement)
    }

    fn lower_case_clause(
        &mut self,
        node: &SyntaxNode,
        body_children: &[GoStatementListItem],
    ) -> Result<AstNodeId, AstError> {
        let case = required(
            GoCase::downcast_from(node.clone()).ok(),
            "switch block",
            "Case",
        )?;
        let keyword = case.case_token().or_else(|| case.default_token()).ok_or(
            AstError::InconsistentCst {
                context: "case clause",
                expected: "case or default",
            },
        )?;
        let colon = required(case.colon_token(), "case clause", "colon")?;
        let expressions = case
            .values()
            .map(|value| self.lower_expr(value.syntax()))
            .collect::<Result<Vec<_>, _>>()?;
        let body = self.lower_statement_children(body_children)?;
        let end = body
            .last()
            .and_then(|body| self.ast_node_end(*body))
            .unwrap_or(colon.to());
        let clause = self.ast.push_node(
            GoAstKind::CaseClause,
            GoSourceRange::new(Some(keyword.from()), Some(end)),
        )?;
        self.ast
            .push_position_field(clause, GoAstField::Case, Some(keyword.from()))?;
        self.ast
            .push_nodes_field(clause, GoAstField::List, &expressions)?;
        self.ast
            .push_position_field(clause, GoAstField::Colon, Some(colon.from()))?;
        self.ast.push_nodes_field(clause, GoAstField::Body, &body)?;
        Ok(clause)
    }

    fn lower_comm_clause(
        &mut self,
        node: &SyntaxNode,
        body_children: &[GoStatementListItem],
    ) -> Result<AstNodeId, AstError> {
        let case = required(
            GoCase::downcast_from(node.clone()).ok(),
            "select block",
            "Case",
        )?;
        let keyword = case.case_token().or_else(|| case.default_token()).ok_or(
            AstError::InconsistentCst {
                context: "communication clause",
                expected: "case or default",
            },
        )?;
        let colon = required(case.colon_token(), "communication clause", "colon")?;
        let communication = case
            .values()
            .next()
            .map(|value| self.lower_statement(value.syntax()))
            .transpose()?;
        let body = self.lower_statement_children(body_children)?;
        let end = body
            .last()
            .and_then(|body| self.ast_node_end(*body))
            .unwrap_or(colon.to());
        let clause = self.ast.push_node(
            GoAstKind::CommClause,
            GoSourceRange::new(Some(keyword.from()), Some(end)),
        )?;
        self.ast
            .push_position_field(clause, GoAstField::Case, Some(keyword.from()))?;
        self.ast
            .push_node_field(clause, GoAstField::Comm, communication)?;
        self.ast
            .push_position_field(clause, GoAstField::Colon, Some(colon.from()))?;
        self.ast.push_nodes_field(clause, GoAstField::Body, &body)?;
        Ok(clause)
    }

    fn lower_empty_statement(
        &mut self,
        token: &SyntaxNode,
        implicit: bool,
    ) -> Result<AstNodeId, AstError> {
        let end = if implicit { token.from() } else { token.to() };
        let statement = self.ast.push_node(
            GoAstKind::EmptyStmt,
            GoSourceRange::new(Some(token.from()), Some(end)),
        )?;
        self.ast
            .push_position_field(statement, GoAstField::Semicolon, Some(token.from()))?;
        self.ast
            .push_bool_field(statement, GoAstField::Implicit, implicit)?;
        Ok(statement)
    }

    fn lower_implicit_empty(&mut self, position: TextSize) -> Result<AstNodeId, AstError> {
        let statement = self.ast.push_node(
            GoAstKind::EmptyStmt,
            GoSourceRange::new(Some(position), Some(position)),
        )?;
        self.ast
            .push_position_field(statement, GoAstField::Semicolon, Some(position))?;
        self.ast
            .push_bool_field(statement, GoAstField::Implicit, true)?;
        Ok(statement)
    }

    fn node_text<'node>(&self, node: &'node SyntaxNode) -> Result<&'source str, AstError> {
        self.source_slice(node.range())
    }

    fn source_slice(&self, range: TextRange) -> Result<&'source str, AstError> {
        self.source
            .get(usize::from(range.start())..usize::from(range.end()))
            .ok_or(AstError::InvalidSourceRange(range))
    }
}

fn kind(node: &SyntaxNode) -> Option<GoKind> {
    <GoLanguage as SyntaxLanguage>::kind(node)
}

fn required<T>(
    value: Option<T>,
    context: &'static str,
    expected: &'static str,
) -> Result<T, AstError> {
    value.ok_or(AstError::InconsistentCst { context, expected })
}

fn token_for(source: &str) -> Option<GoToken> {
    Some(match source {
        "+" => GoToken::Add,
        "-" => GoToken::Sub,
        "*" => GoToken::Mul,
        "/" => GoToken::Quo,
        "%" => GoToken::Rem,
        "&" => GoToken::And,
        "|" => GoToken::Or,
        "^" => GoToken::Xor,
        "<<" => GoToken::Shl,
        ">>" => GoToken::Shr,
        "&^" => GoToken::AndNot,
        "+=" => GoToken::AddAssign,
        "-=" => GoToken::SubAssign,
        "*=" => GoToken::MulAssign,
        "/=" => GoToken::QuoAssign,
        "%=" => GoToken::RemAssign,
        "&=" => GoToken::AndAssign,
        "|=" => GoToken::OrAssign,
        "^=" => GoToken::XorAssign,
        "<<=" => GoToken::ShlAssign,
        ">>=" => GoToken::ShrAssign,
        "&^=" => GoToken::AndNotAssign,
        "&&" => GoToken::Land,
        "||" => GoToken::Lor,
        "<-" => GoToken::Arrow,
        "++" => GoToken::Inc,
        "--" => GoToken::Dec,
        "==" => GoToken::Eql,
        "<" => GoToken::Lss,
        ">" => GoToken::Gtr,
        "=" => GoToken::Assign,
        "!" => GoToken::Not,
        "!=" => GoToken::Neq,
        "<=" => GoToken::Leq,
        ">=" => GoToken::Geq,
        ":=" => GoToken::Define,
        "..." => GoToken::Ellipsis,
        "~" => GoToken::Tilde,
        _ => return None,
    })
}

fn binary_token_for(source: &str) -> Option<GoToken> {
    let token = token_for(source)?;
    matches!(
        token,
        GoToken::Mul
            | GoToken::Quo
            | GoToken::Rem
            | GoToken::Shl
            | GoToken::Shr
            | GoToken::And
            | GoToken::AndNot
            | GoToken::Add
            | GoToken::Sub
            | GoToken::Or
            | GoToken::Xor
            | GoToken::Eql
            | GoToken::Lss
            | GoToken::Gtr
            | GoToken::Neq
            | GoToken::Leq
            | GoToken::Geq
            | GoToken::Land
            | GoToken::Lor
    )
    .then_some(token)
}

fn line_break_count(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut count = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                count += 1;
                index += 2;
            }
            b'\r' | b'\n' => {
                count += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    count
}

fn number_token(source: &str) -> GoToken {
    if source.ends_with('i') {
        return GoToken::Imag;
    }

    let hexadecimal = source.starts_with("0x") || source.starts_with("0X");
    let float_marker = if hexadecimal {
        |byte| matches!(byte, b'.' | b'p' | b'P')
    } else {
        |byte| matches!(byte, b'.' | b'e' | b'E')
    };
    if source.bytes().any(float_marker) {
        GoToken::Float
    } else {
        GoToken::Int
    }
}

fn reject_recovery_tree(tree: &Tree) -> Result<(), AstError> {
    let mut cursor = tree.cursor(IterMode::INCLUDE_ANONYMOUS);
    loop {
        if cursor.node_type().is_error() {
            return Err(AstError::RecoveryTree);
        }
        if !cursor.next(true) {
            return Ok(());
        }
    }
}
