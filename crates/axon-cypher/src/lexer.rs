//! Tokenizer for the Cypher subset.

use crate::error::CypherError;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Punctuation
    LParen,    // (
    RParen,    // )
    LBracket,  // [
    RBracket,  // ]
    LBrace,    // {
    RBrace,    // }
    Comma,     // ,
    Colon,     // :
    Dot,       // .
    Pipe,      // |
    DotDot,    // ..
    Star,      // *
    Dollar,    // $

    // Relationship arrows / dashes
    Dash,      // -
    DashGt,    // ->
    LtDash,    // <-

    // Comparison
    Eq,        // =
    NotEq,     // <> or !=
    Lt,        // <
    LtEq,      // <=
    Gt,        // >
    GtEq,      // >=

    // Keywords (case-insensitive in source; canonical-cased here)
    KwMatch,
    KwOptional,
    KwWhere,
    KwReturn,
    KwDistinct,
    KwAs,
    KwOrder,
    KwBy,
    KwAsc,
    KwDesc,
    KwLimit,
    KwSkip,
    KwAnd,
    KwOr,
    KwNot,
    KwExists,
    KwIs,
    KwNull,
    KwTrue,
    KwFalse,
    KwIn,
    KwContains,
    KwStarts,
    KwEnds,
    KwWith,

    // Literals and identifiers
    Identifier(String),
    StringLit(String),
    IntLit(i64),
    FloatLit(f64),

    // Sentinel
    Eof,
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, CypherError> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    let mut line = 1usize;
    let mut line_start = 0usize;

    while let Some(&(idx, ch)) = chars.peek() {
        let column = idx - line_start + 1;

        if ch.is_whitespace() {
            if ch == '\n' {
                line += 1;
                line_start = idx + 1;
            }
            chars.next();
            continue;
        }

        // Line comments: //
        if ch == '/' {
            let mut clone = chars.clone();
            clone.next();
            if let Some(&(_, '/')) = clone.peek() {
                while let Some(&(_, c)) = chars.peek() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                }
                continue;
            }
        }

        if ch.is_ascii_digit() {
            tokens.push(read_number(&mut chars, line, column, &mut line_start)?);
            continue;
        }

        if ch == '\'' || ch == '"' {
            tokens.push(read_string(&mut chars, line, column, &mut line_start, &mut line)?);
            continue;
        }

        if ch == '_' || ch.is_alphabetic() {
            tokens.push(read_identifier_or_keyword(&mut chars, line, column));
            continue;
        }

        // Single- and multi-char punctuation
        match ch {
            '(' => { chars.next(); tokens.push(Token { kind: TokenKind::LParen, line, column }); }
            ')' => { chars.next(); tokens.push(Token { kind: TokenKind::RParen, line, column }); }
            '[' => { chars.next(); tokens.push(Token { kind: TokenKind::LBracket, line, column }); }
            ']' => { chars.next(); tokens.push(Token { kind: TokenKind::RBracket, line, column }); }
            '{' => { chars.next(); tokens.push(Token { kind: TokenKind::LBrace, line, column }); }
            '}' => { chars.next(); tokens.push(Token { kind: TokenKind::RBrace, line, column }); }
            ',' => { chars.next(); tokens.push(Token { kind: TokenKind::Comma, line, column }); }
            ':' => { chars.next(); tokens.push(Token { kind: TokenKind::Colon, line, column }); }
            '|' => { chars.next(); tokens.push(Token { kind: TokenKind::Pipe, line, column }); }
            '*' => { chars.next(); tokens.push(Token { kind: TokenKind::Star, line, column }); }
            '$' => { chars.next(); tokens.push(Token { kind: TokenKind::Dollar, line, column }); }
            '=' => { chars.next(); tokens.push(Token { kind: TokenKind::Eq, line, column }); }
            '+' | '%' => {
                return Err(CypherError::Parse {
                    line,
                    column,
                    message: format!("unsupported operator '{}' in V1 subset", ch),
                });
            }
            '.' => {
                chars.next();
                if let Some(&(_, '.')) = chars.peek() {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::DotDot, line, column });
                } else {
                    tokens.push(Token { kind: TokenKind::Dot, line, column });
                }
            }
            '-' => {
                chars.next();
                if let Some(&(_, '>')) = chars.peek() {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::DashGt, line, column });
                } else {
                    tokens.push(Token { kind: TokenKind::Dash, line, column });
                }
            }
            '<' => {
                chars.next();
                match chars.peek() {
                    Some(&(_, '-')) => { chars.next(); tokens.push(Token { kind: TokenKind::LtDash, line, column }); }
                    Some(&(_, '=')) => { chars.next(); tokens.push(Token { kind: TokenKind::LtEq, line, column }); }
                    Some(&(_, '>')) => { chars.next(); tokens.push(Token { kind: TokenKind::NotEq, line, column }); }
                    _ => tokens.push(Token { kind: TokenKind::Lt, line, column }),
                }
            }
            '>' => {
                chars.next();
                if let Some(&(_, '=')) = chars.peek() {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::GtEq, line, column });
                } else {
                    tokens.push(Token { kind: TokenKind::Gt, line, column });
                }
            }
            '!' => {
                chars.next();
                if let Some(&(_, '=')) = chars.peek() {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::NotEq, line, column });
                } else {
                    return Err(CypherError::Parse {
                        line,
                        column,
                        message: "unexpected '!' (did you mean '!='?)".to_string(),
                    });
                }
            }
            other => {
                return Err(CypherError::Parse {
                    line,
                    column,
                    message: format!("unexpected character: {:?}", other),
                });
            }
        }
    }

    tokens.push(Token { kind: TokenKind::Eof, line, column: 0 });
    Ok(tokens)
}

fn read_number(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    line: usize,
    column: usize,
    _line_start: &mut usize,
) -> Result<Token, CypherError> {
    let mut s = String::new();
    let mut is_float = false;
    while let Some(&(_, c)) = chars.peek() {
        if c.is_ascii_digit() {
            s.push(c);
            chars.next();
        } else if c == '.' {
            // Look ahead: only consume '.' as a decimal point if followed
            // by a digit. `n.field` and `*1..3` must NOT be eaten here.
            let mut clone = chars.clone();
            clone.next();
            match clone.peek() {
                Some(&(_, c2)) if c2.is_ascii_digit() => {
                    is_float = true;
                    s.push('.');
                    chars.next();
                }
                _ => break,
            }
        } else {
            break;
        }
    }

    if is_float {
        s.parse::<f64>()
            .map(|f| Token { kind: TokenKind::FloatLit(f), line, column })
            .map_err(|e| CypherError::Parse {
                line,
                column,
                message: format!("invalid float literal: {}", e),
            })
    } else {
        s.parse::<i64>()
            .map(|i| Token { kind: TokenKind::IntLit(i), line, column })
            .map_err(|e| CypherError::Parse {
                line,
                column,
                message: format!("invalid integer literal: {}", e),
            })
    }
}

fn read_string(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    line: usize,
    column: usize,
    line_start: &mut usize,
    cur_line: &mut usize,
) -> Result<Token, CypherError> {
    let (_, quote) = chars.next().expect("string start guaranteed by caller");
    let mut s = String::new();
    loop {
        match chars.next() {
            Some((_, c)) if c == quote => {
                return Ok(Token { kind: TokenKind::StringLit(s), line, column });
            }
            Some((_, '\\')) => {
                match chars.next() {
                    Some((_, 'n')) => s.push('\n'),
                    Some((_, 't')) => s.push('\t'),
                    Some((_, 'r')) => s.push('\r'),
                    Some((_, '\\')) => s.push('\\'),
                    Some((_, '\'')) => s.push('\''),
                    Some((_, '"')) => s.push('"'),
                    Some((_, c)) => s.push(c),
                    None => {
                        return Err(CypherError::Parse {
                            line,
                            column,
                            message: "unterminated escape in string literal".to_string(),
                        });
                    }
                }
            }
            Some((idx, '\n')) => {
                *cur_line += 1;
                *line_start = idx + 1;
                s.push('\n');
            }
            Some((_, c)) => s.push(c),
            None => {
                return Err(CypherError::Parse {
                    line,
                    column,
                    message: "unterminated string literal".to_string(),
                });
            }
        }
    }
}

fn read_identifier_or_keyword(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    line: usize,
    column: usize,
) -> Token {
    let mut s = String::new();
    while let Some(&(_, c)) = chars.peek() {
        if c == '_' || c.is_alphanumeric() {
            s.push(c);
            chars.next();
        } else {
            break;
        }
    }

    let kind = match s.to_ascii_uppercase().as_str() {
        "MATCH" => TokenKind::KwMatch,
        "OPTIONAL" => TokenKind::KwOptional,
        "WHERE" => TokenKind::KwWhere,
        "RETURN" => TokenKind::KwReturn,
        "DISTINCT" => TokenKind::KwDistinct,
        "AS" => TokenKind::KwAs,
        "ORDER" => TokenKind::KwOrder,
        "BY" => TokenKind::KwBy,
        "ASC" | "ASCENDING" => TokenKind::KwAsc,
        "DESC" | "DESCENDING" => TokenKind::KwDesc,
        "LIMIT" => TokenKind::KwLimit,
        "SKIP" => TokenKind::KwSkip,
        "AND" => TokenKind::KwAnd,
        "OR" => TokenKind::KwOr,
        "NOT" => TokenKind::KwNot,
        "EXISTS" => TokenKind::KwExists,
        "IS" => TokenKind::KwIs,
        "NULL" => TokenKind::KwNull,
        "TRUE" => TokenKind::KwTrue,
        "FALSE" => TokenKind::KwFalse,
        "IN" => TokenKind::KwIn,
        "CONTAINS" => TokenKind::KwContains,
        "STARTS" => TokenKind::KwStarts,
        "ENDS" => TokenKind::KwEnds,
        "WITH" => TokenKind::KwWith,
        _ => TokenKind::Identifier(s),
    };

    Token { kind, line, column }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<TokenKind> {
        tokenize(input).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn lex_keywords_case_insensitive() {
        assert_eq!(
            kinds("match Match MATCH"),
            vec![TokenKind::KwMatch, TokenKind::KwMatch, TokenKind::KwMatch, TokenKind::Eof],
        );
    }

    #[test]
    fn lex_punctuation_and_arrows() {
        assert_eq!(
            kinds("(a)-[:R]->(b)<-[:S]-(c)"),
            vec![
                TokenKind::LParen,
                TokenKind::Identifier("a".into()),
                TokenKind::RParen,
                TokenKind::Dash,
                TokenKind::LBracket,
                TokenKind::Colon,
                TokenKind::Identifier("R".into()),
                TokenKind::RBracket,
                TokenKind::DashGt,
                TokenKind::LParen,
                TokenKind::Identifier("b".into()),
                TokenKind::RParen,
                TokenKind::LtDash,
                TokenKind::LBracket,
                TokenKind::Colon,
                TokenKind::Identifier("S".into()),
                TokenKind::RBracket,
                TokenKind::Dash,
                TokenKind::LParen,
                TokenKind::Identifier("c".into()),
                TokenKind::RParen,
                TokenKind::Eof,
            ],
        );
    }

    #[test]
    fn lex_numbers_and_property_access() {
        // *1..3 must NOT consume the dots into a float
        assert_eq!(
            kinds("*1..3 n.field 2.5"),
            vec![
                TokenKind::Star,
                TokenKind::IntLit(1),
                TokenKind::DotDot,
                TokenKind::IntLit(3),
                TokenKind::Identifier("n".into()),
                TokenKind::Dot,
                TokenKind::Identifier("field".into()),
                TokenKind::FloatLit(2.5),
                TokenKind::Eof,
            ],
        );
    }

    #[test]
    fn lex_strings_with_escapes() {
        assert_eq!(
            kinds(r#"'hello' "world\n\"esc\"""#),
            vec![
                TokenKind::StringLit("hello".into()),
                TokenKind::StringLit("world\n\"esc\"".into()),
                TokenKind::Eof,
            ],
        );
    }

    #[test]
    fn lex_comparisons() {
        assert_eq!(
            kinds("= <> != < <= > >="),
            vec![
                TokenKind::Eq,
                TokenKind::NotEq,
                TokenKind::NotEq,
                TokenKind::Lt,
                TokenKind::LtEq,
                TokenKind::Gt,
                TokenKind::GtEq,
                TokenKind::Eof,
            ],
        );
    }

    #[test]
    fn lex_line_comment() {
        assert_eq!(
            kinds("MATCH // comment until newline\nRETURN"),
            vec![TokenKind::KwMatch, TokenKind::KwReturn, TokenKind::Eof],
        );
    }
}
