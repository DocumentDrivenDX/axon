//! Recursive-descent parser for the Cypher subset.

use crate::ast::{
    ComparisonOp, Direction, Expression, FunctionArg, HopRange, Literal, LogicalOp, MatchClause,
    NodePattern, PathPattern, PathStep, PropertyEntry, Query, RelationshipPattern, ReturnClause,
    ReturnItem, SortItem, Subquery,
};
use crate::error::CypherError;
use crate::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_query(&mut self) -> Result<Query, CypherError> {
        // Reject explicitly-excluded clauses up front so the error message is
        // about the V1 subset, not about expected MATCH.
        self.reject_unsupported_top_level()?;

        let mut matches = Vec::new();
        loop {
            let optional = self.match_keyword(TokenKind::KwOptional);
            if !self.match_keyword(TokenKind::KwMatch) {
                if optional {
                    return Err(self.error_here("expected MATCH after OPTIONAL"));
                }
                break;
            }
            let patterns = self.parse_path_patterns()?;
            matches.push(MatchClause { optional, patterns });
        }

        if matches.is_empty() {
            return Err(self.error_here("query must begin with MATCH"));
        }

        let where_clause = if self.match_keyword(TokenKind::KwWhere) {
            Some(self.parse_expression()?)
        } else {
            None
        };

        // RETURN required.
        if !self.match_keyword(TokenKind::KwReturn) {
            return Err(self.error_here("expected RETURN"));
        }
        let return_clause = self.parse_return_clause()?;

        let order_by = if self.match_keyword(TokenKind::KwOrder) {
            self.expect_keyword(TokenKind::KwBy, "ORDER BY")?;
            self.parse_sort_items()?
        } else {
            Vec::new()
        };

        let skip = if self.match_keyword(TokenKind::KwSkip) {
            Some(self.parse_u64("SKIP")?)
        } else {
            None
        };

        let limit = if self.match_keyword(TokenKind::KwLimit) {
            Some(self.parse_u64("LIMIT")?)
        } else {
            None
        };

        if !matches!(self.peek(), TokenKind::Eof) {
            return Err(self.error_here("unexpected trailing tokens after query"));
        }

        Ok(Query {
            matches,
            where_clause,
            return_clause,
            order_by,
            skip,
            limit,
        })
    }

    fn reject_unsupported_top_level(&self) -> Result<(), CypherError> {
        if let TokenKind::Identifier(name) = self.peek() {
            let upper = name.to_ascii_uppercase();
            match upper.as_str() {
                "CREATE" | "MERGE" | "SET" | "DELETE" | "DETACH" | "REMOVE" | "CALL"
                | "LOAD" | "USING" | "UNION" | "FOREACH" => {
                    return Err(CypherError::UnsupportedClause(upper));
                }
                _ => {}
            }
        }
        Ok(())
    }

    // ----- Path patterns -----

    fn parse_path_patterns(&mut self) -> Result<Vec<PathPattern>, CypherError> {
        let mut patterns = vec![self.parse_path_pattern()?];
        while self.match_kind(&TokenKind::Comma) {
            patterns.push(self.parse_path_pattern()?);
        }
        Ok(patterns)
    }

    fn parse_path_pattern(&mut self) -> Result<PathPattern, CypherError> {
        let start = self.parse_node_pattern()?;
        let mut steps = Vec::new();
        while matches!(self.peek(), TokenKind::Dash | TokenKind::LtDash) {
            let relationship = self.parse_relationship_pattern()?;
            let node = self.parse_node_pattern()?;
            steps.push(PathStep { relationship, node });
        }
        Ok(PathPattern { start, steps })
    }

    fn parse_node_pattern(&mut self) -> Result<NodePattern, CypherError> {
        self.expect(&TokenKind::LParen, "(")?;
        let variable = if let TokenKind::Identifier(name) = self.peek() {
            let name = name.clone();
            self.advance();
            Some(name)
        } else {
            None
        };
        let label = if self.match_kind(&TokenKind::Colon) {
            Some(self.expect_identifier("label")?)
        } else {
            None
        };
        let properties = if matches!(self.peek(), TokenKind::LBrace) {
            self.parse_property_map()?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::RParen, ")")?;
        Ok(NodePattern { variable, label, properties })
    }

    fn parse_relationship_pattern(&mut self) -> Result<RelationshipPattern, CypherError> {
        // Two valid prefixes: `-[...]->` (outgoing) or `<-[...]-` (incoming).
        // ADR-021's "Either" direction (`-[r]-`) is also accepted.
        let starts_left = self.match_kind(&TokenKind::LtDash);
        if !starts_left {
            self.expect(&TokenKind::Dash, "- or <-")?;
        }

        let mut variable = None;
        let mut types = Vec::new();
        let mut properties = Vec::new();
        let mut range = None;

        if self.match_kind(&TokenKind::LBracket) {
            if let TokenKind::Identifier(name) = self.peek() {
                variable = Some(name.clone());
                self.advance();
            }
            if self.match_kind(&TokenKind::Colon) {
                types.push(self.expect_identifier("relationship type")?);
                while self.match_kind(&TokenKind::Pipe) {
                    types.push(self.expect_identifier("relationship type")?);
                }
            }
            if self.match_kind(&TokenKind::Star) {
                let min = self.expect_u32("variable-length minimum")?;
                self.expect(&TokenKind::DotDot, "..")?;
                let max = self.expect_u32("variable-length maximum")?;
                if min == 0 {
                    return Err(self.error_here("variable-length lower bound must be >= 1"));
                }
                if max < min {
                    return Err(self.error_here("variable-length max must be >= min"));
                }
                range = Some(HopRange { min, max });
            }
            if matches!(self.peek(), TokenKind::LBrace) {
                properties = self.parse_property_map()?;
            }
            self.expect(&TokenKind::RBracket, "]")?;
        }

        let ends_right = self.match_kind(&TokenKind::DashGt);
        if !ends_right {
            self.expect(&TokenKind::Dash, "- or ->")?;
        }

        let direction = match (starts_left, ends_right) {
            (false, true) => Direction::Outgoing,
            (true, false) => Direction::Incoming,
            (false, false) => Direction::Either,
            (true, true) => {
                return Err(self.error_here("relationship cannot be both <- and -> directional"));
            }
        };

        Ok(RelationshipPattern {
            variable,
            direction,
            types,
            properties,
            range,
        })
    }

    fn parse_property_map(&mut self) -> Result<Vec<PropertyEntry>, CypherError> {
        self.expect(&TokenKind::LBrace, "{")?;
        let mut entries = Vec::new();
        if !matches!(self.peek(), TokenKind::RBrace) {
            loop {
                let key = self.expect_identifier("property key")?;
                self.expect(&TokenKind::Colon, ":")?;
                let value = self.parse_expression()?;
                entries.push(PropertyEntry { key, value });
                if !self.match_kind(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RBrace, "}")?;
        Ok(entries)
    }

    // ----- RETURN, ORDER BY -----

    fn parse_return_clause(&mut self) -> Result<ReturnClause, CypherError> {
        let distinct = self.match_keyword(TokenKind::KwDistinct);
        let mut items = vec![self.parse_return_item()?];
        while self.match_kind(&TokenKind::Comma) {
            items.push(self.parse_return_item()?);
        }
        Ok(ReturnClause { distinct, items })
    }

    fn parse_return_item(&mut self) -> Result<ReturnItem, CypherError> {
        let expression = self.parse_expression()?;
        let alias = if self.match_keyword(TokenKind::KwAs) {
            Some(self.expect_identifier("alias")?)
        } else {
            None
        };
        Ok(ReturnItem { expression, alias })
    }

    fn parse_sort_items(&mut self) -> Result<Vec<SortItem>, CypherError> {
        let mut items = vec![self.parse_sort_item()?];
        while self.match_kind(&TokenKind::Comma) {
            items.push(self.parse_sort_item()?);
        }
        Ok(items)
    }

    fn parse_sort_item(&mut self) -> Result<SortItem, CypherError> {
        let expression = self.parse_expression()?;
        let descending = if self.match_keyword(TokenKind::KwDesc) {
            true
        } else {
            self.match_keyword(TokenKind::KwAsc);
            false
        };
        Ok(SortItem { expression, descending })
    }

    // ----- Expressions -----

    fn parse_expression(&mut self) -> Result<Expression, CypherError> {
        self.parse_disjunction()
    }

    fn parse_disjunction(&mut self) -> Result<Expression, CypherError> {
        let mut left = self.parse_conjunction()?;
        while self.match_keyword(TokenKind::KwOr) {
            let right = self.parse_conjunction()?;
            left = Expression::BinaryLogical {
                op: LogicalOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_conjunction(&mut self) -> Result<Expression, CypherError> {
        let mut left = self.parse_negation()?;
        while self.match_keyword(TokenKind::KwAnd) {
            let right = self.parse_negation()?;
            left = Expression::BinaryLogical {
                op: LogicalOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_negation(&mut self) -> Result<Expression, CypherError> {
        if self.match_keyword(TokenKind::KwNot) {
            // NOT EXISTS { ... } is its own form.
            if self.match_keyword(TokenKind::KwExists) {
                let sq = self.parse_subquery()?;
                return Ok(Expression::NotExists(Box::new(sq)));
            }
            let inner = self.parse_negation()?;
            return Ok(Expression::Not(Box::new(inner)));
        }
        if self.match_keyword(TokenKind::KwExists) {
            let sq = self.parse_subquery()?;
            return Ok(Expression::Exists(Box::new(sq)));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expression, CypherError> {
        let left = self.parse_primary()?;

        // IS NULL / IS NOT NULL
        if self.match_keyword(TokenKind::KwIs) {
            let negated = self.match_keyword(TokenKind::KwNot);
            self.expect_keyword(TokenKind::KwNull, "IS [NOT] NULL")?;
            return Ok(Expression::IsNull {
                expression: Box::new(left),
                negated,
            });
        }

        let op = match self.peek() {
            TokenKind::Eq => Some(ComparisonOp::Eq),
            TokenKind::NotEq => Some(ComparisonOp::NotEq),
            TokenKind::Lt => Some(ComparisonOp::Lt),
            TokenKind::LtEq => Some(ComparisonOp::LtEq),
            TokenKind::Gt => Some(ComparisonOp::Gt),
            TokenKind::GtEq => Some(ComparisonOp::GtEq),
            TokenKind::KwIn => Some(ComparisonOp::In),
            TokenKind::KwContains => Some(ComparisonOp::Contains),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            let right = self.parse_primary()?;
            return Ok(Expression::Comparison {
                op,
                left: Box::new(left),
                right: Box::new(right),
            });
        }

        // STARTS WITH / ENDS WITH (two-token operators)
        if matches!(self.peek(), TokenKind::KwStarts) {
            self.advance();
            self.expect_keyword(TokenKind::KwWith, "STARTS WITH")?;
            let right = self.parse_primary()?;
            return Ok(Expression::Comparison {
                op: ComparisonOp::StartsWith,
                left: Box::new(left),
                right: Box::new(right),
            });
        }
        if matches!(self.peek(), TokenKind::KwEnds) {
            self.advance();
            self.expect_keyword(TokenKind::KwWith, "ENDS WITH")?;
            let right = self.parse_primary()?;
            return Ok(Expression::Comparison {
                op: ComparisonOp::EndsWith,
                left: Box::new(left),
                right: Box::new(right),
            });
        }

        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expression, CypherError> {
        match self.peek().clone() {
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expression()?;
                self.expect(&TokenKind::RParen, ")")?;
                Ok(inner)
            }
            TokenKind::Dollar => {
                self.advance();
                let name = self.expect_identifier("parameter name")?;
                Ok(Expression::Parameter(name))
            }
            TokenKind::IntLit(i) => {
                self.advance();
                Ok(Expression::Literal(Literal::Integer(i)))
            }
            TokenKind::FloatLit(f) => {
                self.advance();
                Ok(Expression::Literal(Literal::Float(f)))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(Expression::Literal(Literal::String(s)))
            }
            TokenKind::KwNull => {
                self.advance();
                Ok(Expression::Literal(Literal::Null))
            }
            TokenKind::KwTrue => {
                self.advance();
                Ok(Expression::Literal(Literal::Bool(true)))
            }
            TokenKind::KwFalse => {
                self.advance();
                Ok(Expression::Literal(Literal::Bool(false)))
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !matches!(self.peek(), TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expression()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket, "]")?;
                Ok(Expression::Literal(Literal::List(items)))
            }
            TokenKind::Identifier(name) => {
                self.advance();
                // Function call?
                if matches!(self.peek(), TokenKind::LParen) {
                    self.advance();
                    let mut arguments = Vec::new();
                    if !matches!(self.peek(), TokenKind::RParen) {
                        loop {
                            if self.match_kind(&TokenKind::Star) {
                                arguments.push(FunctionArg::Star);
                            } else {
                                arguments.push(FunctionArg::Expression(self.parse_expression()?));
                            }
                            if !self.match_kind(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen, ")")?;
                    return Ok(Expression::FunctionCall { name, arguments });
                }
                // Property access: name.path[.path]*
                if self.match_kind(&TokenKind::Dot) {
                    let mut path = vec![self.expect_identifier("property")?];
                    while self.match_kind(&TokenKind::Dot) {
                        path.push(self.expect_identifier("property")?);
                    }
                    return Ok(Expression::Property {
                        variable: name,
                        path,
                    });
                }
                Ok(Expression::Variable(name))
            }
            other => Err(self.error_here(&format!("unexpected token in expression: {:?}", other))),
        }
    }

    fn parse_subquery(&mut self) -> Result<Subquery, CypherError> {
        self.expect(&TokenKind::LBrace, "{")?;
        let mut matches = Vec::new();
        loop {
            let optional = self.match_keyword(TokenKind::KwOptional);
            if !self.match_keyword(TokenKind::KwMatch) {
                if optional {
                    return Err(self.error_here("expected MATCH after OPTIONAL"));
                }
                break;
            }
            let patterns = self.parse_path_patterns()?;
            matches.push(MatchClause { optional, patterns });
        }
        if matches.is_empty() {
            return Err(self.error_here("EXISTS subquery must contain at least one MATCH"));
        }
        let where_clause = if self.match_keyword(TokenKind::KwWhere) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        self.expect(&TokenKind::RBrace, "}")?;
        Ok(Subquery {
            matches,
            where_clause,
        })
    }

    // ----- Token helpers -----

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn match_kind(&mut self, expected: &TokenKind) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn match_keyword(&mut self, kw: TokenKind) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(&kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &TokenKind, label: &str) -> Result<Token, CypherError> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            Ok(self.advance())
        } else {
            Err(self.error_here(&format!("expected {}", label)))
        }
    }

    fn expect_keyword(&mut self, kw: TokenKind, label: &str) -> Result<(), CypherError> {
        if self.match_keyword(kw) {
            Ok(())
        } else {
            Err(self.error_here(&format!("expected {}", label)))
        }
    }

    fn expect_identifier(&mut self, label: &str) -> Result<String, CypherError> {
        match self.peek().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.error_here(&format!("expected identifier ({})", label))),
        }
    }

    fn parse_u64(&mut self, label: &str) -> Result<u64, CypherError> {
        match self.peek().clone() {
            TokenKind::IntLit(i) if i >= 0 => {
                self.advance();
                Ok(i as u64)
            }
            _ => Err(self.error_here(&format!("expected non-negative integer ({})", label))),
        }
    }

    fn expect_u32(&mut self, label: &str) -> Result<u32, CypherError> {
        match self.peek().clone() {
            TokenKind::IntLit(i) => match u32::try_from(i) {
                Ok(v) => {
                    self.advance();
                    Ok(v)
                }
                Err(_) => Err(self.error_here(&format!("expected non-negative u32 ({})", label))),
            },
            _ => Err(self.error_here(&format!("expected non-negative u32 ({})", label))),
        }
    }

    fn error_here(&self, message: &str) -> CypherError {
        let tok = &self.tokens[self.pos];
        CypherError::Parse {
            line: tok.line,
            column: tok.column,
            message: message.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse(input: &str) -> Result<Query, CypherError> {
        Parser::new(tokenize(input)?).parse_query()
    }

    #[test]
    fn parses_minimal_match_return() {
        let q = parse("MATCH (n) RETURN n").unwrap();
        assert_eq!(q.matches.len(), 1);
        assert_eq!(q.matches[0].patterns.len(), 1);
        assert_eq!(q.return_clause.items.len(), 1);
    }

    #[test]
    fn parses_label_and_property_predicate() {
        let q = parse("MATCH (b:Bead {status: 'open'}) RETURN b").unwrap();
        let pat = &q.matches[0].patterns[0].start;
        assert_eq!(pat.variable.as_deref(), Some("b"));
        assert_eq!(pat.label.as_deref(), Some("Bead"));
        assert_eq!(pat.properties.len(), 1);
        assert_eq!(pat.properties[0].key, "status");
    }

    #[test]
    fn parses_directed_relationship_and_variable_length() {
        let q = parse("MATCH (a)-[:DEPENDS_ON*1..3]->(b) RETURN a, b").unwrap();
        let path = &q.matches[0].patterns[0];
        assert_eq!(path.steps.len(), 1);
        let rel = &path.steps[0].relationship;
        assert!(matches!(rel.direction, Direction::Outgoing));
        assert_eq!(rel.types, vec!["DEPENDS_ON"]);
        assert_eq!(rel.range, Some(HopRange { min: 1, max: 3 }));
    }

    #[test]
    fn parses_relationship_alternation_and_inbound() {
        let q = parse("MATCH (a)<-[:BLOCKS|DEPENDS_ON]-(b) RETURN b").unwrap();
        let rel = &q.matches[0].patterns[0].steps[0].relationship;
        assert!(matches!(rel.direction, Direction::Incoming));
        assert_eq!(rel.types, vec!["BLOCKS", "DEPENDS_ON"]);
    }

    #[test]
    fn parses_ddx_ready_queue_query() {
        // The load-bearing test: the DDx use case must parse successfully.
        let q = parse(
            r"
            MATCH (b:DdxBead {status: 'open'})
            WHERE NOT EXISTS {
                MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
                WHERE d.status <> 'closed'
            }
            RETURN b
            ORDER BY b.priority DESC, b.updated_at DESC
            LIMIT 100
        ",
        )
        .unwrap();

        assert_eq!(q.matches.len(), 1);
        assert!(q.where_clause.is_some());
        assert_eq!(q.order_by.len(), 2);
        assert!(q.order_by[0].descending);
        assert!(q.order_by[1].descending);
        assert_eq!(q.limit, Some(100));

        match q.where_clause.as_ref().unwrap() {
            Expression::NotExists(sq) => {
                assert_eq!(sq.matches.len(), 1);
                assert!(sq.where_clause.is_some());
            }
            other => panic!("expected NotExists, got {:?}", other),
        }
    }

    #[test]
    fn parses_aggregations_and_aliases() {
        let q = parse(
            "MATCH (i:Invoice) WHERE i.status = 'done' RETURN i.assignee AS who, count(*) AS n",
        )
        .unwrap();
        assert_eq!(q.return_clause.items.len(), 2);
        assert_eq!(q.return_clause.items[0].alias.as_deref(), Some("who"));
        assert_eq!(q.return_clause.items[1].alias.as_deref(), Some("n"));
        match &q.return_clause.items[1].expression {
            Expression::FunctionCall { name, arguments } => {
                assert_eq!(name, "count");
                assert!(matches!(arguments[0], FunctionArg::Star));
            }
            other => panic!("expected count(*), got {:?}", other),
        }
    }

    #[test]
    fn parses_and_or_not() {
        let q = parse(
            "MATCH (n) WHERE n.x > 1 AND (n.y < 5 OR NOT n.z = 'a') RETURN n",
        )
        .unwrap();
        assert!(q.where_clause.is_some());
    }

    #[test]
    fn parses_string_predicates() {
        let q = parse(
            "MATCH (n) WHERE n.title STARTS WITH 'A' AND n.body CONTAINS 'foo' RETURN n",
        )
        .unwrap();
        q.where_clause.expect("where present");
    }

    #[test]
    fn parses_is_null() {
        let q = parse("MATCH (n) WHERE n.deleted_at IS NULL RETURN n").unwrap();
        match q.where_clause.unwrap() {
            Expression::IsNull { negated, .. } => assert!(!negated),
            other => panic!("expected IsNull, got {:?}", other),
        }

        let q = parse("MATCH (n) WHERE n.deleted_at IS NOT NULL RETURN n").unwrap();
        match q.where_clause.unwrap() {
            Expression::IsNull { negated, .. } => assert!(negated),
            other => panic!("expected IsNull negated, got {:?}", other),
        }
    }

    #[test]
    fn parses_parameters() {
        let q = parse("MATCH (n {id: $id}) RETURN n").unwrap();
        let prop = &q.matches[0].patterns[0].start.properties[0];
        match &prop.value {
            Expression::Parameter(name) => assert_eq!(name, "id"),
            other => panic!("expected parameter, got {:?}", other),
        }
    }

    #[test]
    fn rejects_create_clause() {
        let err = parse("CREATE (n:Bead) RETURN n").unwrap_err();
        assert!(matches!(err, CypherError::UnsupportedClause(s) if s == "CREATE"));
    }

    #[test]
    fn rejects_merge_clause() {
        let err = parse("MERGE (n) RETURN n").unwrap_err();
        assert!(matches!(err, CypherError::UnsupportedClause(_)));
    }

    #[test]
    fn rejects_unbounded_quantifier() {
        // The lexer accepts `*1..` as IntLit + Dot ... but our parser expects
        // a number after `..`. Test that we reject it at parse time.
        let err = parse("MATCH (a)-[:R*1..]->(b) RETURN b").unwrap_err();
        assert!(matches!(err, CypherError::Parse { .. }));
    }

    #[test]
    fn rejects_invalid_quantifier_min_zero() {
        let err = parse("MATCH (a)-[:R*0..3]->(b) RETURN b").unwrap_err();
        assert!(matches!(err, CypherError::Parse { .. }));
    }

    #[test]
    fn rejects_quantifier_max_less_than_min() {
        let err = parse("MATCH (a)-[:R*5..2]->(b) RETURN b").unwrap_err();
        assert!(matches!(err, CypherError::Parse { .. }));
    }

    #[test]
    fn parses_optional_match() {
        let q = parse("MATCH (a) OPTIONAL MATCH (a)-[:R]->(b) RETURN a, b").unwrap();
        assert_eq!(q.matches.len(), 2);
        assert!(!q.matches[0].optional);
        assert!(q.matches[1].optional);
    }

    #[test]
    fn parses_skip_and_limit() {
        let q = parse("MATCH (n) RETURN n SKIP 10 LIMIT 5").unwrap();
        assert_eq!(q.skip, Some(10));
        assert_eq!(q.limit, Some(5));
    }

    #[test]
    fn parses_distinct() {
        let q = parse("MATCH (n) RETURN DISTINCT n.label").unwrap();
        assert!(q.return_clause.distinct);
    }

    #[test]
    fn rejects_query_without_match() {
        let err = parse("RETURN 1").unwrap_err();
        assert!(matches!(err, CypherError::Parse { .. }));
    }

    #[test]
    fn rejects_query_without_return() {
        let err = parse("MATCH (n)").unwrap_err();
        assert!(matches!(err, CypherError::Parse { .. }));
    }
}
