use std::collections::BTreeMap;

use itertools::Itertools;
use lazy_static::lazy_static;
use miette::{bail, ensure, miette, IntoDiagnostic, Result};
use pest::prec_climber::{Operator, PrecClimber};
use smartstring::{LazyCompact, SmartString};

use crate::data::expr::{get_op, Expr};
use crate::data::functions::{
    OP_ADD, OP_AND, OP_CONCAT, OP_DIV, OP_EQ, OP_GE, OP_GT, OP_LE, OP_LIST, OP_LT, OP_MINUS,
    OP_MOD, OP_MUL, OP_NEGATE, OP_NEQ, OP_OR, OP_POW, OP_SUB,
};
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::parse::{Pair, Rule};

lazy_static! {
    static ref PREC_CLIMBER: PrecClimber<Rule> = {
        use pest::prec_climber::Assoc::*;

        PrecClimber::new(vec![
            Operator::new(Rule::op_or, Left),
            Operator::new(Rule::op_and, Left),
            Operator::new(Rule::op_gt, Left)
                | Operator::new(Rule::op_lt, Left)
                | Operator::new(Rule::op_ge, Left)
                | Operator::new(Rule::op_le, Left),
            Operator::new(Rule::op_mod, Left),
            Operator::new(Rule::op_eq, Left) | Operator::new(Rule::op_ne, Left),
            Operator::new(Rule::op_add, Left)
                | Operator::new(Rule::op_sub, Left)
                | Operator::new(Rule::op_concat, Left),
            Operator::new(Rule::op_mul, Left) | Operator::new(Rule::op_div, Left),
            Operator::new(Rule::op_pow, Right),
        ])
    };
}

pub(crate) fn build_expr(pair: Pair<'_>, param_pool: &BTreeMap<String, DataValue>) -> Result<Expr> {
    PREC_CLIMBER.climb(
        pair.into_inner(),
        |v| build_unary(v, param_pool),
        build_expr_infix,
    )
}

fn build_expr_infix(lhs: Result<Expr>, op: Pair<'_>, rhs: Result<Expr>) -> Result<Expr> {
    let args = vec![lhs?, rhs?];
    let op = match op.as_rule() {
        Rule::op_add => &OP_ADD,
        Rule::op_sub => &OP_SUB,
        Rule::op_mul => &OP_MUL,
        Rule::op_div => &OP_DIV,
        Rule::op_mod => &OP_MOD,
        Rule::op_pow => &OP_POW,
        Rule::op_eq => &OP_EQ,
        Rule::op_ne => &OP_NEQ,
        Rule::op_gt => &OP_GT,
        Rule::op_ge => &OP_GE,
        Rule::op_lt => &OP_LT,
        Rule::op_le => &OP_LE,
        Rule::op_concat => &OP_CONCAT,
        Rule::op_or => &OP_OR,
        Rule::op_and => &OP_AND,
        _ => unreachable!(),
    };
    Ok(Expr::Apply {
        op,
        args: args.into(),
    })
}

fn build_unary(pair: Pair<'_>, param_pool: &BTreeMap<String, DataValue>) -> Result<Expr> {
    Ok(match pair.as_rule() {
        Rule::expr => build_unary(pair.into_inner().next().unwrap(), param_pool)?,
        Rule::grouping => build_expr(pair.into_inner().next().unwrap(), param_pool)?,
        Rule::unary => {
            let s = pair.as_str();
            let mut inner = pair.into_inner();
            let p = inner.next().unwrap();
            let op = p.as_rule();
            match op {
                Rule::term => build_unary(p, param_pool)?,
                Rule::var => Expr::Binding {
                    var: Symbol::from(s),
                    tuple_pos: None,
                },
                Rule::param => {
                    let param_str = s.strip_prefix('$').unwrap();
                    Expr::Const {
                        val: param_pool
                            .get(param_str)
                            .ok_or_else(|| miette!("required param '{}' not found", param_str))?
                            .clone(),
                    }
                }
                Rule::minus => {
                    let inner = build_unary(inner.next().unwrap(), param_pool)?;
                    Expr::Apply {
                        op: &OP_MINUS,
                        args: [inner].into(),
                    }
                }
                Rule::negate => {
                    let inner = build_unary(inner.next().unwrap(), param_pool)?;
                    Expr::Apply {
                        op: &OP_NEGATE,
                        args: [inner].into(),
                    }
                }
                Rule::pos_int => {
                    let i = s.replace('_', "").parse::<i64>().into_diagnostic()?;
                    Expr::Const {
                        val: DataValue::from(i),
                    }
                }
                Rule::hex_pos_int => {
                    let i = parse_int(s, 16);
                    Expr::Const {
                        val: DataValue::from(i),
                    }
                }
                Rule::octo_pos_int => {
                    let i = parse_int(s, 8);
                    Expr::Const {
                        val: DataValue::from(i),
                    }
                }
                Rule::bin_pos_int => {
                    let i = parse_int(s, 2);
                    Expr::Const {
                        val: DataValue::from(i),
                    }
                }
                Rule::dot_float | Rule::sci_float => {
                    let f = s.replace('_', "").parse::<f64>().into_diagnostic()?;
                    Expr::Const {
                        val: DataValue::from(f),
                    }
                }
                Rule::null => Expr::Const {
                    val: DataValue::Null,
                },
                Rule::boolean => Expr::Const {
                    val: DataValue::Bool(s == "true"),
                },
                Rule::quoted_string | Rule::s_quoted_string | Rule::raw_string => {
                    let s = parse_string(p)?;
                    Expr::Const {
                        val: DataValue::Str(s),
                    }
                }
                Rule::list => {
                    let mut collected = vec![];
                    for p in p.into_inner() {
                        collected.push(build_expr(p, param_pool)?)
                    }
                    Expr::Apply {
                        op: &OP_LIST,
                        args: collected.into(),
                    }
                }
                Rule::apply => {
                    let mut p = p.into_inner();
                    let ident = p.next().unwrap().as_str();
                    let mut args: Box<_> = p
                        .next()
                        .unwrap()
                        .into_inner()
                        .map(|v| build_expr(v, param_pool))
                        .try_collect()?;
                    let op = get_op(ident).ok_or_else(|| miette!("op not found: {}", ident))?;
                    op.post_process_args(&mut args);
                    if op.vararg {
                        ensure!(op.min_arity <= args.len(), "args too short for {}", ident);
                    } else {
                        ensure!(op.min_arity == args.len(), "args not right for {}", ident);
                    }
                    Expr::Apply {
                        op,
                        args: args.into(),
                    }
                }
                Rule::grouping => build_expr(p.into_inner().next().unwrap(), param_pool)?,
                r => unreachable!("Encountered unknown op {:?}", r),
            }
        }
        _ => {
            println!("Unhandled rule {:?}", pair.as_rule());
            unimplemented!()
        }
    })
}

pub(crate) fn parse_int(s: &str, radix: u32) -> i64 {
    i64::from_str_radix(&s[2..].replace('_', ""), radix).unwrap()
}

pub(crate) fn parse_string(pair: Pair<'_>) -> Result<SmartString<LazyCompact>> {
    match pair.as_rule() {
        Rule::quoted_string => Ok(parse_quoted_string(pair)?),
        Rule::s_quoted_string => Ok(parse_s_quoted_string(pair)?),
        Rule::raw_string => Ok(parse_raw_string(pair)?),
        Rule::ident => Ok(SmartString::from(pair.as_str())),
        t => unreachable!("{:?}", t),
    }
}

fn parse_quoted_string(pair: Pair<'_>) -> Result<SmartString<LazyCompact>> {
    let pairs = pair.into_inner().next().unwrap().into_inner();
    let mut ret = SmartString::new();
    for pair in pairs {
        let s = pair.as_str();
        match s {
            r#"\""# => ret.push('"'),
            r"\\" => ret.push('\\'),
            r"\/" => ret.push('/'),
            r"\b" => ret.push('\x08'),
            r"\f" => ret.push('\x0c'),
            r"\n" => ret.push('\n'),
            r"\r" => ret.push('\r'),
            r"\t" => ret.push('\t'),
            s if s.starts_with(r"\u") => {
                let code = parse_int(s, 16) as u32;
                let ch =
                    char::from_u32(code).ok_or_else(|| miette!("invalid UTF8 code {}", code))?;
                ret.push(ch);
            }
            s if s.starts_with('\\') => {
                bail!("invalid escape sequence {}", s);
            }
            s => ret.push_str(s),
        }
    }
    Ok(ret)
}

fn parse_s_quoted_string(pair: Pair<'_>) -> Result<SmartString<LazyCompact>> {
    let pairs = pair.into_inner().next().unwrap().into_inner();
    let mut ret = SmartString::new();
    for pair in pairs {
        let s = pair.as_str();
        match s {
            r#"\'"# => ret.push('\''),
            r"\\" => ret.push('\\'),
            r"\/" => ret.push('/'),
            r"\b" => ret.push('\x08'),
            r"\f" => ret.push('\x0c'),
            r"\n" => ret.push('\n'),
            r"\r" => ret.push('\r'),
            r"\t" => ret.push('\t'),
            s if s.starts_with(r"\u") => {
                let code = parse_int(s, 16) as u32;
                let ch =
                    char::from_u32(code).ok_or_else(|| miette!("invalid UTF8 code {}", code))?;
                ret.push(ch);
            }
            s if s.starts_with('\\') => {
                bail!("invalid escape sequence {}", s);
            }
            s => ret.push_str(s),
        }
    }
    Ok(ret)
}

fn parse_raw_string(pair: Pair<'_>) -> Result<SmartString<LazyCompact>> {
    Ok(SmartString::from(
        pair.into_inner().into_iter().next().unwrap().as_str(),
    ))
}