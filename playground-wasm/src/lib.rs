use std::collections::{HashMap, HashSet};
use std::fmt;

use patmat::{
    AtomicIntersection, Decomposition, MatchArm, MatchInput, ReachabilityWarning, Space,
    SpaceContext, SpaceKind, SpaceOperations, check_match,
};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Span {
    start: usize,
    end: usize,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    ok: bool,
    error: String,
    span: Option<Span>,
}

#[derive(Debug)]
struct DslError {
    message: String,
    span: Option<Span>,
}

impl DslError {
    fn new(message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    fn at(message: impl Into<String>, span: Span) -> Self {
        Self::new(message, Some(span))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Program {
    types: Vec<TypeDecl>,
    match_input: MatchBlock,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TypeDecl {
    name: String,
    params: Vec<String>,
    constructors: Vec<ConstructorDecl>,
    span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConstructorDecl {
    name: String,
    fields: Vec<TypeExpr>,
    span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MatchBlock {
    scrutinee: TypeExpr,
    arms: Vec<Pattern>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TypeExpr {
    Named(String, Vec<TypeExpr>, Span),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Pattern {
    Wildcard {
        span: Span,
    },
    Or {
        alternatives: Vec<Pattern>,
        span: Span,
    },
    Constructor {
        name: String,
        args: Vec<Pattern>,
        span: Span,
    },
}

impl Pattern {
    fn span(&self) -> Span {
        match self {
            Self::Wildcard { span } | Self::Or { span, .. } | Self::Constructor { span, .. } => {
                *span
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    KeywordType,
    KeywordMatch,
    Pipe,
    Equal,
    Colon,
    Comma,
    LParen,
    RParen,
    Less,
    Greater,
    Underscore,
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    kind: TokenKind,
    span: Span,
}

fn lex(source: &str) -> Result<Vec<Token>, DslError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        let start = index;
        let kind = match byte {
            b'|' => {
                index += 1;
                TokenKind::Pipe
            }
            b'=' => {
                index += 1;
                TokenKind::Equal
            }
            b':' => {
                index += 1;
                TokenKind::Colon
            }
            b',' => {
                index += 1;
                TokenKind::Comma
            }
            b'(' => {
                index += 1;
                TokenKind::LParen
            }
            b')' => {
                index += 1;
                TokenKind::RParen
            }
            b'<' => {
                index += 1;
                TokenKind::Less
            }
            b'>' => {
                index += 1;
                TokenKind::Greater
            }
            b'_' => {
                index += 1;
                TokenKind::Underscore
            }
            b if is_ident_start(b) => {
                index += 1;
                while index < bytes.len() && is_ident_continue(bytes[index]) {
                    index += 1;
                }
                let text = &source[start..index];
                match text {
                    "type" => TokenKind::KeywordType,
                    "match" => TokenKind::KeywordMatch,
                    _ => TokenKind::Ident(text.to_owned()),
                }
            }
            _ => {
                return Err(DslError::at(
                    format!("Unexpected character `{}`", byte as char),
                    Span {
                        start,
                        end: start + 1,
                    },
                ));
            }
        };

        tokens.push(Token {
            kind,
            span: Span { start, end: index },
        });
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span {
            start: source.len(),
            end: source.len(),
        },
    });
    Ok(tokens)
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn parse_program(&mut self) -> Result<Program, DslError> {
        let mut types = Vec::new();
        while self.at_kind(&TokenKind::KeywordType) {
            types.push(self.parse_type_decl()?);
        }

        if !self.at_kind(&TokenKind::KeywordMatch) {
            return Err(DslError::at("Expected a match block", self.current().span));
        }
        let match_input = self.parse_match_block()?;

        if !self.at_kind(&TokenKind::Eof) {
            return Err(DslError::at(
                "Only one match block is supported",
                self.current().span,
            ));
        }

        Ok(Program { types, match_input })
    }

    fn parse_type_decl(&mut self) -> Result<TypeDecl, DslError> {
        let start = self
            .expect_kind(&TokenKind::KeywordType, "Expected `type`")?
            .start;
        let (name, _) = self.expect_ident("Expected a type name")?;
        let params = if self.consume_kind(&TokenKind::Less).is_some() {
            let mut params = Vec::new();
            loop {
                let (param, span) = self.expect_ident("Expected a type parameter")?;
                if !params.insert_unique(param.clone()) {
                    return Err(DslError::at(
                        format!("Duplicate type parameter `{param}`"),
                        span,
                    ));
                }
                if self.consume_kind(&TokenKind::Comma).is_some() {
                    continue;
                }
                self.expect_kind(&TokenKind::Greater, "Expected `>`")?;
                break;
            }
            params
        } else {
            Vec::new()
        };

        self.expect_kind(&TokenKind::Equal, "Expected `=`")?;
        let mut constructors = Vec::new();
        while self.consume_kind(&TokenKind::Pipe).is_some() {
            constructors.push(self.parse_constructor_decl()?);
        }

        if constructors.is_empty() {
            return Err(DslError::at(
                format!("Type `{name}` must declare at least one constructor"),
                self.current().span,
            ));
        }

        let end = constructors
            .last()
            .map_or(start, |constructor| constructor.span.end);
        Ok(TypeDecl {
            name,
            params,
            constructors,
            span: Span { start, end },
        })
    }

    fn parse_constructor_decl(&mut self) -> Result<ConstructorDecl, DslError> {
        let (name, name_span) = self.expect_ident("Expected a constructor name")?;
        let mut span = name_span;
        let fields = if self.consume_kind(&TokenKind::LParen).is_some() {
            let mut fields = Vec::new();
            if !self.at_kind(&TokenKind::RParen) {
                loop {
                    fields.push(self.parse_type_expr()?);
                    if self.consume_kind(&TokenKind::Comma).is_some() {
                        continue;
                    }
                    break;
                }
            }
            span.end = self.expect_kind(&TokenKind::RParen, "Expected `)`")?.end;
            fields
        } else {
            Vec::new()
        };
        Ok(ConstructorDecl { name, fields, span })
    }

    fn parse_match_block(&mut self) -> Result<MatchBlock, DslError> {
        self.expect_kind(&TokenKind::KeywordMatch, "Expected `match`")?;
        let scrutinee = self.parse_type_expr()?;
        self.expect_kind(&TokenKind::Colon, "Expected `:`")?;

        let mut arms = Vec::new();
        while !self.at_kind(&TokenKind::Eof) {
            if self.at_kind(&TokenKind::KeywordMatch) {
                return Err(DslError::at(
                    "Only one match block is supported",
                    self.current().span,
                ));
            }
            if self.at_kind(&TokenKind::KeywordType) {
                return Err(DslError::at(
                    "Type declarations must appear before the match block",
                    self.current().span,
                ));
            }
            arms.push(self.parse_pattern()?);
        }

        Ok(MatchBlock { scrutinee, arms })
    }

    fn parse_type_expr(&mut self) -> Result<TypeExpr, DslError> {
        let (name, name_span) = self.expect_ident("Expected a type expression")?;
        let mut span = name_span;
        let args = if self.consume_kind(&TokenKind::Less).is_some() {
            let mut args = Vec::new();
            if self.at_kind(&TokenKind::Greater) {
                return Err(DslError::at(
                    "Expected a type argument",
                    self.current().span,
                ));
            }
            loop {
                args.push(self.parse_type_expr()?);
                if self.consume_kind(&TokenKind::Comma).is_some() {
                    continue;
                }
                span.end = self.expect_kind(&TokenKind::Greater, "Expected `>`")?.end;
                break;
            }
            args
        } else {
            Vec::new()
        };
        Ok(TypeExpr::Named(name, args, span))
    }

    fn parse_pattern(&mut self) -> Result<Pattern, DslError> {
        let mut alternatives = vec![self.parse_primary_pattern()?];
        while self.consume_kind(&TokenKind::Pipe).is_some() {
            alternatives.push(self.parse_primary_pattern()?);
        }

        if alternatives.len() == 1 {
            Ok(alternatives.pop().expect("one alternative was parsed"))
        } else {
            let start = alternatives
                .first()
                .expect("or pattern alternatives must not be empty")
                .span()
                .start;
            let end = alternatives
                .last()
                .expect("or pattern alternatives must not be empty")
                .span()
                .end;
            Ok(Pattern::Or {
                alternatives,
                span: Span { start, end },
            })
        }
    }

    fn parse_primary_pattern(&mut self) -> Result<Pattern, DslError> {
        if let Some(span) = self.consume_kind(&TokenKind::Underscore) {
            return Ok(Pattern::Wildcard { span });
        }

        let (name, name_span) = self.expect_ident("Expected a pattern")?;
        let mut span = name_span;
        let args = if self.consume_kind(&TokenKind::LParen).is_some() {
            let mut args = Vec::new();
            if !self.at_kind(&TokenKind::RParen) {
                loop {
                    args.push(self.parse_pattern()?);
                    if self.consume_kind(&TokenKind::Comma).is_some() {
                        continue;
                    }
                    break;
                }
            }
            span.end = self.expect_kind(&TokenKind::RParen, "Expected `)`")?.end;
            args
        } else {
            Vec::new()
        };
        Ok(Pattern::Constructor { name, args, span })
    }

    fn current(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn at_kind(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind)
    }

    fn consume_kind(&mut self, kind: &TokenKind) -> Option<Span> {
        if self.at_kind(kind) {
            let span = self.current().span;
            self.position += 1;
            Some(span)
        } else {
            None
        }
    }

    fn expect_kind(&mut self, kind: &TokenKind, message: &str) -> Result<Span, DslError> {
        self.consume_kind(kind)
            .ok_or_else(|| DslError::at(message, self.current().span))
    }

    fn expect_ident(&mut self, message: &str) -> Result<(String, Span), DslError> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Ident(name) => {
                self.position += 1;
                Ok((name, token.span))
            }
            _ => Err(DslError::at(message, token.span)),
        }
    }
}

trait InsertUnique<T> {
    fn insert_unique(&mut self, value: T) -> bool;
}

impl<T: PartialEq> InsertUnique<T> for Vec<T> {
    fn insert_unique(&mut self, value: T) -> bool {
        if self.contains(&value) {
            false
        } else {
            self.push(value);
            true
        }
    }
}

fn parse(source: &str) -> Result<Program, DslError> {
    Parser::new(lex(source)?).parse_program()
}

#[derive(Clone, Debug)]
struct Model {
    types: Vec<TypeDecl>,
    type_by_name: HashMap<String, usize>,
    constructor_names: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum RuntimeType {
    Adt {
        name: String,
        args: Vec<RuntimeType>,
    },
    Variant {
        adt: String,
        adt_args: Vec<RuntimeType>,
        constructor: String,
        fields: Vec<RuntimeType>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RuntimeExtractor {
    adt: String,
    constructor: String,
}

impl Model {
    fn build(program: &Program) -> Result<Self, DslError> {
        let mut type_by_name = HashMap::new();
        let mut constructor_names = HashSet::new();

        for (type_index, type_decl) in program.types.iter().enumerate() {
            if type_by_name
                .insert(type_decl.name.clone(), type_index)
                .is_some()
            {
                return Err(DslError::at(
                    format!("Duplicate type `{}`", type_decl.name),
                    type_decl.span,
                ));
            }

            let mut local_constructors = HashSet::new();
            for constructor in &type_decl.constructors {
                if !local_constructors.insert(constructor.name.clone()) {
                    return Err(DslError::at(
                        format!(
                            "Duplicate constructor `{}` in type `{}`",
                            constructor.name, type_decl.name
                        ),
                        constructor.span,
                    ));
                }

                constructor_names.insert(constructor.name.clone());
            }
        }

        let model = Self {
            types: program.types.clone(),
            type_by_name,
            constructor_names,
        };

        for type_decl in &model.types {
            let params: HashSet<_> = type_decl.params.iter().cloned().collect();
            for constructor in &type_decl.constructors {
                for field in &constructor.fields {
                    model.validate_type_expr(field, &params)?;
                    if field.references_type(&type_decl.name) {
                        return Err(DslError::at(
                            format!("Recursive type `{}` is not supported", type_decl.name),
                            field.span(),
                        ));
                    }
                }
            }
        }

        Ok(model)
    }

    fn validate_type_expr(
        &self,
        expr: &TypeExpr,
        params: &HashSet<String>,
    ) -> Result<(), DslError> {
        let TypeExpr::Named(name, args, span) = expr;
        if params.contains(name) {
            if !args.is_empty() {
                return Err(DslError::at(
                    format!("Type parameter `{name}` cannot have arguments"),
                    *span,
                ));
            }
            return Ok(());
        }

        let Some(type_decl) = self.type_decl(name) else {
            return Err(DslError::at(format!("Unknown type `{name}`"), *span));
        };

        if type_decl.params.len() != args.len() {
            return Err(DslError::at(
                format!(
                    "Type `{}` expects {} type argument(s), got {}",
                    name,
                    type_decl.params.len(),
                    args.len()
                ),
                *span,
            ));
        }

        for arg in args {
            self.validate_type_expr(arg, params)?;
        }

        Ok(())
    }

    fn resolve_closed_type(&self, expr: &TypeExpr) -> Result<RuntimeType, DslError> {
        self.resolve_type(expr, &HashMap::new())
    }

    fn resolve_type(
        &self,
        expr: &TypeExpr,
        substitutions: &HashMap<String, RuntimeType>,
    ) -> Result<RuntimeType, DslError> {
        let TypeExpr::Named(name, args, span) = expr;
        if let Some(substitution) = substitutions.get(name) {
            if !args.is_empty() {
                return Err(DslError::at(
                    format!("Type parameter `{name}` cannot have arguments"),
                    *span,
                ));
            }
            return Ok(substitution.clone());
        }

        let Some(type_decl) = self.type_decl(name) else {
            return Err(DslError::at(format!("Unknown type `{name}`"), *span));
        };
        if type_decl.params.len() != args.len() {
            return Err(DslError::at(
                format!(
                    "Type `{}` expects {} type argument(s), got {}",
                    name,
                    type_decl.params.len(),
                    args.len()
                ),
                *span,
            ));
        }

        Ok(RuntimeType::Adt {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| self.resolve_type(arg, substitutions))
                .collect::<Result<_, _>>()?,
        })
    }

    fn type_decl(&self, name: &str) -> Option<&TypeDecl> {
        self.type_by_name
            .get(name)
            .map(|&type_index| &self.types[type_index])
    }

    fn constructor_in_type(
        &self,
        expected: &RuntimeType,
        constructor_name: &str,
        span: Span,
    ) -> Result<ResolvedConstructor<'_>, DslError> {
        let RuntimeType::Adt { name, args } = expected else {
            return Err(DslError::at(
                format!("Constructor `{constructor_name}` cannot match this scrutinee type"),
                span,
            ));
        };

        let Some(type_index) = self.type_by_name.get(name).copied() else {
            return Err(DslError::at(format!("Unknown type `{name}`"), span));
        };
        let type_decl = &self.types[type_index];
        let Some(constructor) = type_decl
            .constructors
            .iter()
            .find(|constructor| constructor.name == constructor_name)
        else {
            if self.constructor_names.contains(constructor_name) {
                return Err(DslError::at(
                    format!("Constructor `{constructor_name}` cannot match `{expected}`"),
                    span,
                ));
            }
            return Err(DslError::at(
                format!("Unknown constructor `{constructor_name}`"),
                span,
            ));
        };

        Ok(ResolvedConstructor {
            type_decl,
            constructor,
            adt_args: args.clone(),
        })
    }

    fn variant_type(
        &self,
        type_decl: &TypeDecl,
        constructor: &ConstructorDecl,
        adt_args: &[RuntimeType],
    ) -> RuntimeType {
        let substitutions: HashMap<_, _> = type_decl
            .params
            .iter()
            .cloned()
            .zip(adt_args.iter().cloned())
            .collect();
        let fields = constructor
            .fields
            .iter()
            .map(|field| {
                self.resolve_type(field, &substitutions)
                    .expect("validated constructor field types must resolve")
            })
            .collect();

        RuntimeType::Variant {
            adt: type_decl.name.clone(),
            adt_args: adt_args.to_vec(),
            constructor: constructor.name.clone(),
            fields,
        }
    }

    fn lower_pattern(
        &self,
        context: &mut SpaceContext<RuntimeType, RuntimeExtractor>,
        pattern: &Pattern,
        expected: &RuntimeType,
    ) -> Result<Space<RuntimeType, RuntimeExtractor>, DslError> {
        match pattern {
            Pattern::Wildcard { .. } => Ok(context.of_type(expected.clone())),
            Pattern::Or { alternatives, .. } => {
                let spaces = alternatives
                    .iter()
                    .map(|alternative| self.lower_pattern(context, alternative, expected))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(context.union(spaces))
            }
            Pattern::Constructor { name, args, span } => {
                let resolved = self.constructor_in_type(expected, name, *span)?;
                let variant =
                    self.variant_type(resolved.type_decl, resolved.constructor, &resolved.adt_args);
                let RuntimeType::Variant { fields, .. } = &variant else {
                    unreachable!("variant_type always creates a variant")
                };
                if fields.len() != args.len() {
                    return Err(DslError::at(
                        format!(
                            "Constructor `{name}` expects {} argument(s), got {}",
                            fields.len(),
                            args.len()
                        ),
                        *span,
                    ));
                }
                if fields.is_empty() {
                    return Ok(context.of_type(variant));
                }

                let parameters = args
                    .iter()
                    .zip(fields)
                    .map(|(arg, field_type)| self.lower_pattern(context, arg, field_type))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(context.product(
                    variant,
                    RuntimeExtractor {
                        adt: resolved.type_decl.name.clone(),
                        constructor: resolved.constructor.name.clone(),
                    },
                    parameters,
                ))
            }
        }
    }

    fn lower_match(
        &self,
        program: &Program,
        context: &mut SpaceContext<RuntimeType, RuntimeExtractor>,
    ) -> Result<(RuntimeType, MatchInput<RuntimeType, RuntimeExtractor>), DslError> {
        let scrutinee = self.resolve_closed_type(&program.match_input.scrutinee)?;
        let scrutinee_space = context.of_type(scrutinee.clone());
        let arms = program
            .match_input
            .arms
            .iter()
            .map(|pattern| {
                let pattern_space = self.lower_pattern(context, pattern, &scrutinee)?;
                if matches!(pattern, Pattern::Wildcard { .. }) {
                    Ok(MatchArm::wildcard(pattern_space))
                } else {
                    Ok(MatchArm::new(pattern_space))
                }
            })
            .collect::<Result<Vec<_>, DslError>>()?;

        Ok((scrutinee, MatchInput::new(scrutinee_space, arms)))
    }

    fn response_types(&self) -> Vec<TypeResponse> {
        self.types
            .iter()
            .map(|type_decl| TypeResponse {
                name: type_decl.name.clone(),
                params: type_decl.params.clone(),
                constructors: type_decl
                    .constructors
                    .iter()
                    .map(|constructor| {
                        if constructor.fields.is_empty() {
                            constructor.name.clone()
                        } else {
                            let fields = constructor
                                .fields
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ");
                            format!("{}({fields})", constructor.name)
                        }
                    })
                    .collect(),
            })
            .collect()
    }
}

struct ResolvedConstructor<'a> {
    type_decl: &'a TypeDecl,
    constructor: &'a ConstructorDecl,
    adt_args: Vec<RuntimeType>,
}

impl TypeExpr {
    fn span(&self) -> Span {
        let Self::Named(_, _, span) = self;
        *span
    }

    fn references_type(&self, target: &str) -> bool {
        let Self::Named(name, args, _) = self;
        name == target || args.iter().any(|arg| arg.references_type(target))
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Named(name, args, _) = self;
        if args.is_empty() {
            f.write_str(name)
        } else {
            write!(
                f,
                "{}<{}>",
                name,
                args.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

impl fmt::Display for RuntimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeType::Adt { name, args } => {
                if args.is_empty() {
                    f.write_str(name)
                } else {
                    write!(
                        f,
                        "{}<{}>",
                        name,
                        args.iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            RuntimeType::Variant {
                constructor,
                fields,
                ..
            } => {
                if fields.is_empty() {
                    f.write_str(constructor)
                } else {
                    write!(
                        f,
                        "{}({})",
                        constructor,
                        fields
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
        }
    }
}

impl fmt::Display for RuntimeExtractor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.constructor)
    }
}

impl SpaceOperations for Model {
    type Type = RuntimeType;
    type Extractor = RuntimeExtractor;

    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type> {
        let RuntimeType::Adt { name, args } = value_type else {
            return Decomposition::NotDecomposable;
        };

        let Some(type_decl) = self.type_decl(name) else {
            return Decomposition::Empty;
        };

        Decomposition::parts(
            type_decl
                .constructors
                .iter()
                .map(|constructor| self.variant_type(type_decl, constructor, args))
                .collect(),
        )
    }

    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
        if left == right {
            return true;
        }
        matches!(
            (left, right),
            (
                RuntimeType::Variant { adt: left_adt, adt_args: left_args, .. },
                RuntimeType::Adt { name: right_adt, args: right_args }
            ) if left_adt == right_adt && left_args == right_args
        )
    }

    fn extractors_are_equivalent(&self, left: &Self::Extractor, right: &Self::Extractor) -> bool {
        left == right
    }

    fn covering_extractor_parameter_types(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> Option<Vec<Self::Type>> {
        match scrutinee_type {
            RuntimeType::Variant {
                adt,
                constructor,
                fields,
                ..
            } if adt == &extractor.adt
                && constructor == &extractor.constructor
                && fields.len() == arity =>
            {
                Some(fields.clone())
            }
            _ => None,
        }
    }

    fn intersect_atomic_types(
        &self,
        left: &Self::Type,
        right: &Self::Type,
    ) -> AtomicIntersection<Self::Type> {
        if left == right {
            AtomicIntersection::Type(left.clone())
        } else {
            AtomicIntersection::Empty
        }
    }
}

#[derive(Debug, Serialize)]
struct SuccessResponse {
    ok: bool,
    scrutinee: String,
    #[serde(rename = "isExhaustive")]
    is_exhaustive: bool,
    uncovered: Vec<String>,
    warnings: Vec<WarningResponse>,
    arms: Vec<ArmResponse>,
    types: Vec<TypeResponse>,
}

#[derive(Debug, Serialize)]
struct ArmResponse {
    index: usize,
    source: String,
    span: Span,
    space: String,
    reachable: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
enum WarningResponse {
    Unreachable {
        #[serde(rename = "armIndex")]
        arm_index: usize,
        #[serde(rename = "coveringArmIndices")]
        covering_arm_indices: Vec<usize>,
    },
    OnlyNull {
        #[serde(rename = "armIndex")]
        arm_index: usize,
        #[serde(rename = "coveringArmIndices")]
        covering_arm_indices: Vec<usize>,
    },
}

#[derive(Debug, Serialize)]
struct TypeResponse {
    name: String,
    params: Vec<String>,
    constructors: Vec<String>,
}

#[wasm_bindgen]
pub fn analyze_dsl(source: String) -> Result<JsValue, JsValue> {
    match analyze_source(&source) {
        Ok(response) => serde_wasm_bindgen::to_value(&response)
            .map_err(|error| JsValue::from_str(&error.to_string())),
        Err(error) => {
            let response = ErrorResponse {
                ok: false,
                error: error.message,
                span: error.span,
            };
            serde_wasm_bindgen::to_value(&response)
                .map_err(|error| JsValue::from_str(&error.to_string()))
        }
    }
}

fn analyze_source(source: &str) -> Result<SuccessResponse, DslError> {
    let program = parse(source)?;
    let model = Model::build(&program)?;
    let mut context = SpaceContext::new();
    let (scrutinee, match_input) = model.lower_match(&program, &mut context)?;
    let analysis = check_match(&model, &mut context, &match_input);
    let unreachable_indices: HashSet<_> = analysis
        .reachability_warnings
        .iter()
        .filter_map(|warning| match warning {
            ReachabilityWarning::Unreachable { arm_index, .. } => Some(*arm_index),
            ReachabilityWarning::OnlyNull { .. } => None,
        })
        .collect();

    Ok(SuccessResponse {
        ok: true,
        scrutinee: scrutinee.to_string(),
        is_exhaustive: analysis.is_exhaustive(),
        uncovered: analysis
            .uncovered_spaces
            .iter()
            .map(|space| format_space(&context, *space))
            .collect(),
        warnings: analysis
            .reachability_warnings
            .iter()
            .map(WarningResponse::from)
            .collect(),
        arms: program
            .match_input
            .arms
            .iter()
            .enumerate()
            .map(|(index, pattern)| ArmResponse {
                index,
                source: source[pattern.span().start..pattern.span().end]
                    .trim()
                    .to_owned(),
                span: pattern.span(),
                space: format_space(&context, match_input.arms[index].pattern_space),
                reachable: !unreachable_indices.contains(&index),
            })
            .collect(),
        types: model.response_types(),
    })
}

impl From<&ReachabilityWarning> for WarningResponse {
    fn from(warning: &ReachabilityWarning) -> Self {
        match warning {
            ReachabilityWarning::Unreachable {
                arm_index,
                covering_arm_indices,
            } => Self::Unreachable {
                arm_index: *arm_index,
                covering_arm_indices: covering_arm_indices.clone(),
            },
            ReachabilityWarning::OnlyNull {
                arm_index,
                covering_arm_indices,
            } => Self::OnlyNull {
                arm_index: *arm_index,
                covering_arm_indices: covering_arm_indices.clone(),
            },
        }
    }
}

fn format_space(
    context: &SpaceContext<RuntimeType, RuntimeExtractor>,
    space: Space<RuntimeType, RuntimeExtractor>,
) -> String {
    match space.kind(context) {
        SpaceKind::Empty => "empty".to_owned(),
        SpaceKind::Type(kind) => kind.value_type.to_string(),
        SpaceKind::Product(kind) => {
            if kind.parameters.is_empty() {
                kind.extractor.to_string()
            } else {
                format!(
                    "{}({})",
                    kind.extractor,
                    kind.parameters
                        .iter()
                        .map(|space| format_space(context, *space))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        SpaceKind::Union(members) => members
            .iter()
            .map(|space| format_space(context, *space))
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const BOOL: &str = "type Bool =\n  | true\n  | false\n\nmatch Bool:\n  true\n  false\n";

    fn json(source: &str) -> Value {
        serde_json::to_value(analyze_source(source).unwrap()).unwrap()
    }

    fn error(source: &str) -> String {
        match analyze_source(source) {
            Ok(_) => panic!("expected error"),
            Err(error) => error.message,
        }
    }

    #[test]
    fn parses_adt_declarations() {
        let program = parse(BOOL).unwrap();
        assert_eq!(program.types[0].name, "Bool");
        assert_eq!(program.types[0].constructors.len(), 2);
    }

    #[test]
    fn parses_generic_adt_declarations() {
        let source = "type Option<T> =\n  | Some(T)\n  | None\n\nmatch Option<Option<T>>:\n";
        let program = parse(source).unwrap();
        assert_eq!(program.types[0].params, vec!["T"]);
        assert_eq!(program.types[0].constructors[0].fields.len(), 1);
    }

    #[test]
    fn parses_match_block() {
        let program = parse(BOOL).unwrap();
        assert_eq!(program.match_input.arms.len(), 2);
    }

    #[test]
    fn parses_nested_generic_type_expressions() {
        let source = "type Bool =\n  | true\n  | false\n\ntype Option<T> =\n  | Some(T)\n  | None\n\ntype Result<T, E> =\n  | Ok(T)\n  | Err(E)\n\nmatch Result<Bool, Option<Bool>>:\n";
        let program = parse(source).unwrap();
        assert_eq!(
            program.match_input.scrutinee.to_string(),
            "Result<Bool, Option<Bool>>"
        );
    }

    #[test]
    fn analyzes_exhaustive_bool() {
        let result = json(BOOL);
        assert_eq!(result["ok"], true);
        assert_eq!(result["isExhaustive"], true);
        assert_eq!(result["uncovered"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn analyzes_non_exhaustive_bool() {
        let result = json("type Bool =\n  | true\n  | false\n\nmatch Bool:\n  true\n");
        assert_eq!(result["isExhaustive"], false);
        assert_eq!(result["uncovered"], serde_json::json!(["false"]));
    }

    #[test]
    fn analyzes_exhaustive_option_bool() {
        let source = "type Bool =\n  | true\n  | false\n\ntype Option<T> =\n  | Some(T)\n  | None\n\nmatch Option<Bool>:\n  Some(true)\n  Some(false)\n  None\n";
        assert_eq!(json(source)["isExhaustive"], true);
    }

    #[test]
    fn analyzes_nested_or_pattern() {
        let source = "type Bool =\n  | true\n  | false\n\ntype Option<T> =\n  | Some(T)\n  | None\n\nmatch Option<Bool>:\n  Some(true | false)\n  None\n";
        let result = json(source);
        assert_eq!(result["isExhaustive"], true);
        assert_eq!(result["arms"][0]["space"], "Some(true | false)");
    }

    #[test]
    fn analyzes_top_level_or_pattern() {
        let source = "type Bool =\n  | true\n  | false\n\nmatch Bool:\n  true | false\n";
        assert_eq!(json(source)["isExhaustive"], true);
    }

    #[test]
    fn analyzes_nested_result_option() {
        let source = "type Bool =\n  | true\n  | false\n\ntype Option<T> =\n  | Some(T)\n  | None\n\ntype Result<T, E> =\n  | Ok(T)\n  | Err(E)\n\nmatch Result<Bool, Option<Bool>>:\n  Ok(true)\n  Ok(false)\n  Err(Some(true))\n  Err(Some(false))\n  Err(None)\n";
        assert_eq!(json(source)["isExhaustive"], true);
    }

    #[test]
    fn reports_unreachable_wildcard_duplicate_case() {
        let result = json("type Bool =\n  | true\n  | false\n\nmatch Bool:\n  _\n  true\n");
        assert_eq!(result["isExhaustive"], true);
        assert_eq!(result["arms"][1]["reachable"], false);
        assert_eq!(result["warnings"][0]["kind"], "Unreachable");
        assert_eq!(result["warnings"][0]["armIndex"], 1);
        assert_eq!(
            result["warnings"][0]["coveringArmIndices"],
            serde_json::json!([0])
        );
    }

    #[test]
    fn reports_unknown_type_error() {
        assert!(error("match Nope:\n  _\n").contains("Unknown type `Nope`"));
    }

    #[test]
    fn reports_unknown_constructor_error() {
        assert!(
            error("type Bool =\n  | true\n  | false\n\nmatch Bool:\n  Foo\n")
                .contains("Unknown constructor `Foo`")
        );
    }

    #[test]
    fn reports_wrong_constructor_arity() {
        assert!(
            error("type Bool =\n  | true\n  | false\n\ntype Option<T> =\n  | Some(T)\n  | None\n\nmatch Option<Bool>:\n  Some(true, false)\n")
                .contains("expects 1 argument")
        );
    }

    #[test]
    fn reports_constructor_used_against_wrong_scrutinee_type() {
        assert!(
            error("type Bool =\n  | true\n  | false\n\ntype Option<T> =\n  | Some(T)\n  | None\n\nmatch Bool:\n  Some(true)\n")
                .contains("cannot match `Bool`")
        );
    }
}
