// Copyright (c) 2018 The StructOpt developpers
//
// This work is free. You can redistribute it and/or modify it under
// the terms of the Do What The Fuck You Want To Public License,
// Version 2, as published by Sam Hocevar. See the COPYING file for
// more details.

use std::env;
use quote::Tokens;
use syn::{self, Attribute, MetaNameValue, MetaList, LitStr};

#[derive(Debug)]
pub struct Attrs {
    name: String,
    methods: Vec<Method>,
    parser: (Parser, Tokens),
    has_custom_parser: bool,
    is_subcommand: bool,
}
#[derive(Debug)]
struct Method {
    name: String,
    args: Tokens,
}
#[derive(Debug, PartialEq)]
pub enum Parser {
    FromStr,
    TryFromStr,
    FromOsStr,
    TryFromOsStr,
    FromOccurrences,
}
impl ::std::str::FromStr for Parser {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "from_str" => Ok(Parser::FromStr),
            "try_from_str" => Ok(Parser::TryFromStr),
            "from_os_str" => Ok(Parser::FromOsStr),
            "try_from_os_str" => Ok(Parser::TryFromOsStr),
            "from_occurrences" => Ok(Parser::FromOccurrences),
            _ => Err(format!("unsupported parser {}", s))
        }
    }
}

impl Attrs {
    fn new(attrs: &[Attribute], name: String, methods: Vec<Method>) -> Attrs {
        use Meta::*;
        use NestedMeta::*;
        use Lit::*;

        let mut res = Attrs {
            name: name,
            methods: methods,
            parser: (Parser::TryFromStr, quote!(::std::str::FromStr::from_str)),
            has_custom_parser: false,
            is_subcommand: false,
        };
        let iter = attrs.iter()
            .filter_map(|attr| {
                let path = &attr.path;
                match quote!(#path) == quote!(structopt) {
                    true => Some(
                        attr.interpret_meta()
                            .expect(&format!("invalid structopt syntax: {}", quote!(attr)))
                    ),
                    false => None,
                }
            }).
            flat_map(|m| match m {
                List(l) => l.nested,
                tokens => panic!("unsupported syntax: {}", quote!(#tokens).to_string()),
            })
            .map(|m| match m {
                Meta(m) => m,
                ref tokens => panic!("unsupported syntax: {}", quote!(#tokens).to_string()),
            });
        for attr in iter {
            match attr {
                NameValue(MetaNameValue { ident, lit: Str(ref value), .. })
                    if value.value() == "" => {
                        res.methods = res.methods
                            .into_iter()
                            .filter(|m| m.name == ident.as_ref())
                            .collect();
                    }
                NameValue(MetaNameValue { ident, lit: Str(ref s), .. })
                    if ident == "name" => res.name = s.value(),
                NameValue(MetaNameValue { ident, lit, .. }) => {
                    res.methods.push(Method {
                        name: ident.to_string(),
                        args: quote!(#lit),
                    })
                }
                List(MetaList { ident, ref nested, .. }) if ident == "parse" => {
                    if nested.len() != 1 {
                        panic!("parse must have exactly one argument");
                    }
                    res.has_custom_parser = true;
                    res.parser = match nested[0] {
                        Meta(NameValue(MetaNameValue { ident, lit: Str(ref v), .. })) => {
                            let function: syn::Path = v.parse().expect("parser function path");
                            let parser = ident.as_ref().parse().unwrap();
                            (parser, quote!(#function))
                        }
                        Meta(Word(ref i)) => {
                            use Parser::*;
                            let parser = i.as_ref().parse().unwrap();
                            let function = match parser {
                                FromStr => quote!(::std::convert::From::from),
                                TryFromStr => quote!(::std::str::FromStr::from_str),
                                FromOsStr => quote!(::std::convert::From::from),
                                TryFromOsStr => panic!("cannot omit parser function name with `try_from_os_str`"),
                                FromOccurrences => quote!({|v| v as _}),
                            };
                            (parser, function)
                        }
                        ref l @ _ => panic!("unknown value parser specification: {}", quote!(#l)),
                    };
                }
                List(MetaList { ident, ref nested, .. }) if ident == "raw" => {
                    for method in nested {
                        match *method {
                            Meta(NameValue(MetaNameValue { ident, lit: Str(ref v), .. })) =>
                                res.push_raw_method(ident.as_ref(), v),
                            ref mi @ _ => panic!("unsupported raw entry: {}", quote!(#mi)),
                        }
                    }
                }
                Word(ref w) if w == "subcommand" => res.is_subcommand = true,
                ref i @ List(..) | ref i @ Word(..) =>
                    panic!("unsupported option: {}", quote!(#i)),
            }
        }
        res
    }
    fn push_raw_method(&mut self, name: &str, args: &LitStr) {
        let ts: ::proc_macro2::TokenStream = args.value().parse()
            .expect(&format!("bad parameter {} = {}: the parameter must be valid rust code", name, quote!(#args)));
        self.methods.push(Method {
            name: name.to_string(),
            args: quote!(#(#ts)*),
        })
    }
    fn push_doc_comment(&mut self, attrs: &[Attribute], name: &str) {
        if self.has_method(name) { return; }
        let doc_comments: Vec<_> = attrs.iter()
            .filter_map(|attr| {
                let path = &attr.path;
                match quote!(#path) == quote!(doc) {
                    true => attr.interpret_meta(),
                    false => None,
                }
            })
            .filter_map(|attr| {
                use Meta::*;
                use Lit::*;
                if let NameValue(MetaNameValue { ident, lit: Str(s), .. }) = attr {
                    if ident != "doc" { return None; }
                    let value = s.value();
                    let text = value
                        .trim_left_matches("//!")
                        .trim_left_matches("///")
                        .trim_left_matches("/*!")
                        .trim_left_matches("/**")
                        .trim_right_matches("*/")
                        .trim();
                    Some(text.to_string())
                } else {
                    None
                }
            })
            .collect();
        if doc_comments.is_empty() { return; }
        let arg = doc_comments.join(" ");
        self.methods.push(Method {
            name: name.to_string(),
            args: quote!(#arg),
        });
    }
    pub fn from_struct(attrs: &[Attribute], name: String) -> Attrs {
        let methods =
            [
                ("version", "CARGO_PKG_VERSION"),
                ("about", "CARGO_PKG_DESCRIPTION"),
                ("author", "CARGO_PKG_AUTHORS"),
            ]
            .iter()
            .filter_map(|&(m, v)| env::var(v).ok().and_then(|arg| Some((m, arg))))
            .filter(|&(_, ref arg)| arg.is_empty())
            .map(|(name, arg)| {
                if arg == "author" { arg.replace(":", ", "); }
                Method { name: name.into(), args: quote!(#arg) }
            })
            .collect();
        let mut res = Self::new(attrs, name, methods);
        if res.has_custom_parser {
            panic!("parse attribute is only allowed on fields");
        }
        if res.is_subcommand {
            panic!("subcommand is only allowed on fields");
        }
        res.push_doc_comment(attrs, "about");
        res
    }
    pub fn from_field(field: &syn::Field) -> Attrs {
        let name = field.ident.as_ref().unwrap().as_ref().to_string();
        let mut res = Self::new(&field.attrs, name, vec![]);
        if res.is_subcommand {
            if res.has_custom_parser {
                panic!("parse attribute is not allowed for subcommand");
            }
            if !res.methods.is_empty() {
                panic!("methods in attributes is not allowed for subcommand");
            }
        }
        res.push_doc_comment(&field.attrs, "help");
        res
    }
    pub fn has_method(&self, method: &str) -> bool {
        self.methods.iter().find(|m| m.name == method).is_some()
    }
    pub fn methods(&self) -> Tokens {
        let methods = self.methods.iter().map(|&Method { ref name, ref args }| {
            let name: ::syn::Ident = name.as_str().into();
            quote!( .#name(#args) )
        });
        quote!( #(#methods)* )
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn parser(&self) -> &(Parser, Tokens) {
        &self.parser
    }
    pub fn has_custom_parser(&self) -> bool {
        self.has_custom_parser
    }
    pub fn is_subcommand(&self) -> bool {
        self.is_subcommand
    }
}
