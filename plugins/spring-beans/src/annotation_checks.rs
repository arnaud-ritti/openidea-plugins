//! Annotation misuse in Java code: `@Profile` expressions and `@Async` return
//! types. Quiet for foreign annotations and non-literal arguments.

use crate::checks::spring;
use openidea_plugin::Severity;
use spring_text::finding::Finding;
use spring_text::java::{Annotation, JavaFile, TypeDecl};

const NON_FUTURE_RETURNS: [&str; 18] = [
    "boolean", "byte", "short", "char", "int", "long", "float", "double", "String", "Integer", "Long", "Double",
    "Boolean", "Object", "List", "Set", "Map", "Optional",
];

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Name,
    Not,
    And,
    Or,
    Open,
    Close,
}

fn tokens(value: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut in_name = false;
    for c in value.chars() {
        let token = match c {
            '!' => Token::Not,
            '&' => Token::And,
            '|' => Token::Or,
            '(' => Token::Open,
            ')' => Token::Close,
            c => {
                if !c.is_whitespace() && !in_name {
                    out.push(Token::Name);
                    in_name = true;
                }
                continue;
            }
        };
        in_name = false;
        out.push(token);
    }
    out
}

/// What is wrong with a profile expression such as `a & !b`, if anything.
pub fn profile_expression_problem(value: &str) -> Option<&'static str> {
    let tokens = tokens(value);
    if tokens.is_empty() {
        return Some("A profile name can't be empty: Spring rejects the value when the class is loaded.");
    }
    let mut at = 0;
    if let Err(problem) = expression(&tokens, &mut at) {
        return Some(problem);
    }
    (at < tokens.len()).then_some("A profile expression has a closing parenthesis without an opening one.")
}

fn expression(tokens: &[Token], at: &mut usize) -> Result<(), &'static str> {
    term(tokens, at)?;
    let mut operator = None;
    while let Some(&op) = tokens.get(*at).filter(|t| matches!(t, Token::And | Token::Or)) {
        if operator.is_some_and(|first| first != op) {
            return Err("A profile expression can't mix & and | at one level: wrap one of them in parentheses, as in (a & b) | c.");
        }
        operator = Some(op);
        *at += 1;
        term(tokens, at)?;
    }
    Ok(())
}

fn term(tokens: &[Token], at: &mut usize) -> Result<(), &'static str> {
    let token = tokens.get(*at).copied();
    *at += 1;
    match token {
        Some(Token::Name) => Ok(()),
        Some(Token::Not) => term(tokens, at).map_err(|_| "A ! in a profile expression needs a profile name or a parenthesised expression after it."),
        Some(Token::Open) => {
            expression(tokens, at)?;
            let closed = tokens.get(*at) == Some(&Token::Close);
            *at += 1;
            if closed {
                return Ok(());
            }
            Err("A profile expression has an opening parenthesis without a closing one.")
        }
        Some(Token::Close) => Err("A profile expression has a closing parenthesis without an opening one."),
        _ => Err("A profile expression ends where an operand is expected: add a profile name."),
    }
}

fn profile_values(file: &JavaFile, annotations: &[Annotation], out: &mut Vec<Finding>) {
    let Some(profile) = spring(file, annotations, "Profile") else { return };
    let Some(value) = profile.arg("value") else { return };
    if value.text.contains('+') {
        return;
    }
    for literal in &value.strings {
        if literal.value.contains("${") || literal.value.contains("#{") {
            continue;
        }
        let Some(problem) = profile_expression_problem(&literal.value) else { continue };
        out.push(Finding::new(literal.at, literal.end, "profile-expression", problem).with_severity(Severity::Error));
    }
}

pub fn check_profiles(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    profile_values(file, &ty.annotations, out);
    for method in ty.methods() {
        profile_values(file, &method.annotations, out);
    }
}

pub fn check_async(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    for method in ty.methods() {
        if spring(file, &method.annotations, "Async").is_none() {
            continue;
        }
        let Some(returned) = &method.ty else { continue };
        if !NON_FUTURE_RETURNS.contains(&returned.name.as_str()) {
            continue;
        }
        out.push(Finding::new(
            returned.at,
            returned.at + returned.name.len(),
            "async-return-type",
            format!("An @Async method should return void or a Future: the caller gets null instead of this {}, because the call returns before the work runs.", returned.name),
        ));
    }
}
