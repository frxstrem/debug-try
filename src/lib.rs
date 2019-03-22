#![feature(proc_macro_diagnostic)]
#![feature(proc_macro_span)]

extern crate proc_macro;

use proc_macro::{Diagnostic, TokenStream};
use proc_macro2::Span;

use syn::{
    parse::{self, Parser},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
    visit_mut::{self, VisitMut},
    AttributeArgs, Expr, ExprClosure, ExprTry, ItemFn, Lit, Macro, Meta, NestedMeta, Stmt,
    Token,
};

use quote::ToTokens;

/// `debug_try` is a function attribute macro that will replace any occurence of the `?` try operator
/// with code that prints to standard error whenever an error is propagated.
///
/// The macro works by replacing any occurence of `expr?` with
/// ```ignore
/// expr.map_err(|err| {
///     /* Print error message and location to standard error */;
///     err
/// })?
/// ```
///
/// When an error is propagated, a message similar to this is printed:
/// ```text
/// Error propagated (file.rs:10:30): Some error message
/// ```
///
/// # Arguments
///
/// The macro can be used with or without arguments:
/// ```ignore
/// #[debug_try]
/// #[debug_try(nested = false)]
/// ```
///
/// The following arguments are supported:
/// * `nested`: If true, the macro will transform closures and inner functions as well. By default,
///   this is false.
///
/// # Limitations
///
/// * The macro can only transform functions that return `Result<T, E>` where `E` implements
///   [`Display`](std::fmt::Display).
/// * The macro attribute can only be used on functions, not modules or closures.
/// * The macro will only transform `?` try operators that occur in certain known macros:
///   `println`, `eprintln`, `format`, `write` and `writeln`.
///
/// # Example
///
/// ```
/// use std::{error, fs, io, path};
/// use debug_try::debug_try;
/// # fn main() { my_func(); }
///
/// #[debug_try(nested = true)]
/// fn my_func() -> Result<(), Box<dyn error::Error>> {
///     fn file_size<P: AsRef<path::Path>>(file: P) -> Result<usize, io::Error> {
///         let data = fs::read(file)?;
///         Ok(data.len())
///     }
///
///     println!("file size = {}", file_size("non_existing_file.txt")?);
///     Ok(())
/// }
/// ```
#[proc_macro_attribute]
pub fn debug_try(args: TokenStream, input: TokenStream) -> TokenStream {
    // parse arguments
    let args: AttributeArgs = parse_macro_input!(args);
    let args = match DebugTryArgs::try_from(args) {
        Ok(args) => args,
        Err(diag) => {
            diag.emit();
            return input;
        }
    };

    // parse input
    let input: ItemFn = parse_macro_input!(input);

    // alter input
    debug_try_inner(&args, input.clone())
        .unwrap_or_else(|diags| {
            diags.into_iter().for_each(|diag| diag.emit());
            input
        })
        .into_token_stream()
        .into()
}

fn debug_try_inner(args: &DebugTryArgs, mut input: ItemFn) -> Result<ItemFn, Vec<Diagnostic>> {
    struct Visitor<'a>(&'a DebugTryArgs, Vec<Diagnostic>);
    impl<'a> Visitor<'a> {
        fn push_paser_error(&mut self, err: parse::Error) {
            self.1.push(err.span().unstable().error(err.to_string()))
        }
    }

    impl<'a> VisitMut for Visitor<'a> {
        fn visit_expr_closure_mut(&mut self, i: &mut ExprClosure) {
            let is_nested = self.0.nested.unwrap_or(false);
            if is_nested {
                visit_mut::visit_expr_closure_mut(self, i);
            }
        }

        fn visit_expr_try_mut(&mut self, i: &mut ExprTry) {
            let span: Span = i.question_token.span();

            let file = span
                .unstable()
                .source_file()
                .path()
                .to_string_lossy()
                .into_owned();
            let line_column = span.unstable().start();
            let format_str = format!(
                "Error propagated ({}:{}:{}): {{}}",
                file, line_column.line, line_column.column
            );

            let mut expr = i.expr.clone();
            self.visit_expr_mut(&mut expr);

            i.expr = parse_quote! {
                #expr.map_err(|err| {
                    eprintln!(#format_str, err);
                    err
                })
            };
        }

        fn visit_macro_mut(&mut self, i: &mut Macro) {
            // only substitute in known macros

            const KNOWN: &[&str] = &["println", "eprintln", "format", "write", "writeln"];
            if KNOWN.iter().any(|name| i.path.is_ident(name)) {
                let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
                match parser.parse2(i.tts.clone()) {
                    Ok(mut tree) => {
                        tree.iter_mut().for_each(|item| self.visit_expr_mut(item));
                        i.tts = tree.into_token_stream()
                    }

                    Err(err) => {
                        self.push_paser_error(err);
                    }
                }
            }
        }

        fn visit_stmt_mut(&mut self, i: &mut Stmt) {
            match i {
                Stmt::Item(_) => {
                    if self.0.nested.unwrap_or(false) {
                        visit_mut::visit_stmt_mut(self, i);
                    }
                }

                _ => visit_mut::visit_stmt_mut(self, i),
            }
        }
    }

    let mut visitor = Visitor(args, Vec::new());
    visit_mut::visit_item_fn_mut(&mut visitor, &mut input);

    if visitor.1.is_empty() {
        Ok(input)
    } else {
        Err(visitor.1)
    }
}

#[derive(Default, Debug)]
struct DebugTryArgs {
    nested: Option<bool>,
}

impl DebugTryArgs {
    fn try_from(args: AttributeArgs) -> Result<DebugTryArgs, Diagnostic> {
        let mut result: DebugTryArgs = Default::default();

        for arg in args {
            match arg {
                NestedMeta::Meta(Meta::NameValue(ref nv)) => {
                    let key: &str = &nv.ident.to_string();

                    match key {
                        "nested" => {
                            if result.nested.is_some() {
                                return Err(nv.ident.span().unstable().error("Duplicate argument"));
                            }

                            result.nested = match nv.lit {
                                Lit::Bool(ref bool_lit) => Some(bool_lit.value),
                                _ => {
                                    return Err(nv
                                        .lit
                                        .span()
                                        .unstable()
                                        .error("Expected boolean literal"));
                                }
                            };
                        }
                        _ => return Err(nv.ident.span().unstable().error("Unknown argument")),
                    }
                }
                _ => return Err(arg.span().unstable().error("Expected key-value pair")),
            }
        }

        Ok(result)
    }
}
