extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::Expr;
use syn::Ident;
use syn::Result;
use syn::Token;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::parse_macro_input;
use syn::punctuated::Punctuated;

enum PathPattern {
  Path {
    segments: Vec<SegmentPattern>,
    fully_qualified: bool,
  },
  Wildcard,
}

#[derive(Clone)]
enum SegmentPatternBindingMode {
  Normal,
  Multi,
  Optional,
}

enum SegmentPattern {
  Required(SegmentMatcher),
  Optional(SegmentMatcher),
  Binding {
    span: Span,
    lifetime: bool,
    name: Ident,
    mode: SegmentPatternBindingMode,
  },
}

struct SegmentMatcher {
  ident: Ident,
  args: Option<ArgumentPattern>,
}

enum ArgumentPattern {
  AngleBracketed(Vec<GenericArgumentPattern>),
}

enum GenericArgumentPattern {
  Argument(Box<SegmentMatcher>),
  Wildcard(bool),
  Binding(Span, bool, Ident),
  AssociatedType(Span, Ident, AssociatedTypes),
  PathPattern(Vec<SegmentPattern>),
}

enum AssociatedTypes {
  Wildcard,
  Binding(Span, bool, Ident),
  Argument(Box<SegmentMatcher>),
}

struct PathMatchArms {
  path: Expr,
  arms: Vec<MatchArm>,
}

struct MatchArm {
  patterns: Punctuated<PathPattern, Token![|]>,
  _fat_arrow: Token![=>],
  body: Expr,
  _comma: Option<Token![,]>,
}

// macro body
impl Parse for PathMatchArms {
  fn parse(input: ParseStream) -> Result<Self> {
    let path = input.parse()?;
    input.parse::<Token![,]>()?;

    let mut arms = Vec::new();
    while !input.is_empty() {
      arms.push(input.parse()?);
    }

    Ok(PathMatchArms { path, arms })
  }
}

// foo::bar | hello::world => {}
// ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
impl Parse for MatchArm {
  fn parse(input: ParseStream) -> Result<Self> {
    let patterns = Punctuated::parse_separated_nonempty(input)?;
    let _fat_arrow = input.parse()?;
    let body = input.parse()?;
    let _comma = input.parse().ok();

    Ok(MatchArm {
      patterns,
      _fat_arrow,
      body,
      _comma,
    })
  }
}

// foo::bar | hello::world => {}
// ^^^^^^^^
impl Parse for PathPattern {
  fn parse(input: ParseStream) -> Result<Self> {
    if input.peek(Token![_]) {
      input.parse::<Token![_]>()?;
      return Ok(PathPattern::Wildcard);
    }

    let mut segments = Vec::new();
    let mut fully_qualified = false;

    if input.peek(Token![::]) {
      input.parse::<Token![::]>()?;
      fully_qualified = true;
    }

    loop {
      if input.peek(Token![$]) {
        input.parse::<Token![$]>()?;

        let (lifetime, mut name) = if input.peek(syn::Lifetime) {
          let lt: syn::Lifetime = input.parse()?;
          (true, lt.ident)
        } else {
          (false, input.parse()?)
        };

        let mode = if input.peek(Token![*]) {
          input.parse::<Token![*]>()?;
          SegmentPatternBindingMode::Multi
        } else if input.peek(Token![?]) {
          input.parse::<Token![?]>()?;
          SegmentPatternBindingMode::Optional
        } else {
          SegmentPatternBindingMode::Normal
        };


        let span = name.span();
        name.set_span(Span::call_site());

        segments.push(SegmentPattern::Binding {
          span,
          lifetime,
          name,
          mode,
        });

        if input.peek(Token![::]) {
          input.parse::<Token![::]>()?;
        } else {
          break;
        }
        continue;
      }

      let ident: Ident = input.parse()?;

      let args = if input.peek(Token![<]) {
        Some(input.parse()?)
      } else {
        None
      };

      let optional = if input.peek(Token![?]) {
        input.parse::<Token![?]>()?;
        true
      } else {
        false
      };

      let matcher = SegmentMatcher { ident, args };

      segments.push(if optional {
        SegmentPattern::Optional(matcher)
      } else {
        SegmentPattern::Required(matcher)
      });

      if input.peek(Token![::]) {
        input.parse::<Token![::]>()?;
      } else {
        break;
      }
    }

    Ok(PathPattern::Path {
      segments,
      fully_qualified,
    })
  }
}

// foo::bar<baz> | hello::world => {}
//         ^^^^^
impl Parse for ArgumentPattern {
  fn parse(input: ParseStream) -> Result<Self> {
    input.parse::<Token![<]>()?;
    let mut args = Vec::new();

    loop {
      if input.peek(Token![_]) { // wildcard
        input.parse::<Token![_]>()?;
        args.push(GenericArgumentPattern::Wildcard(false));
      } else if input.peek(syn::Lifetime) { // lifetime wildcard
        let lt: syn::Lifetime = input.parse()?;
        if lt.ident == "_" {
          args.push(GenericArgumentPattern::Wildcard(true));
        } else {
          return Err(syn::Error::new(lt.span(), "expected '_ for lifetime wildcard"));
        }
      } else if input.peek(Token![$]) { // binding
        input.parse::<Token![$]>()?;

        let (lifetime, mut name) = if input.peek(syn::Lifetime) {
          let lt: syn::Lifetime = input.parse()?;
          (true, lt.ident)
        } else {
          (false, input.parse()?)
        };

        let span = name.span();
        name.set_span(Span::call_site());

        args.push(GenericArgumentPattern::Binding(span, lifetime, name));
      } else if input.peek(syn::token::Bracket) { // slice
        let content;
        syn::bracketed!(content in input);

        let slice_ident = Ident::new("__slice__", input.span());
        let mut element_args = Vec::new();

        if content.peek(Token![$]) {
          content.parse::<Token![$]>()?;
          let (lifetime, mut name) = if content.peek(syn::Lifetime) {
            let lt: syn::Lifetime = content.parse()?;
            (true, lt.ident)
          } else {
            (false, content.parse()?)
          };
          let span = name.span();
          name.set_span(Span::call_site());
          element_args.push(GenericArgumentPattern::Binding(span, lifetime, name));
        } else {
          let element_ident: Ident = content.parse()?;
          let element_matcher = SegmentMatcher {
            ident: element_ident,
            args: None,
          };
          element_args.push(GenericArgumentPattern::Argument(Box::new(element_matcher)));
        }

        let slice_matcher = SegmentMatcher {
          ident: slice_ident,
          args: Some(ArgumentPattern::AngleBracketed(element_args)),
        };
        args.push(GenericArgumentPattern::Argument(Box::new(slice_matcher)));
      } else if input.peek(Token![::]) { // path coercion
        input.parse::<Token![::]>()?;

        if input.peek(Token![$]) {
          input.parse::<Token![$]>()?;

          let (lifetime, mut name) = if input.peek(syn::Lifetime) {
            let lt: syn::Lifetime = input.parse()?;
            (true, lt.ident)
          } else {
            (false, input.parse()?)
          };

          let span = name.span();
          name.set_span(Span::call_site());

          let path_segments = vec![SegmentPattern::Binding {
            span,
            lifetime,
            name,
            mode: SegmentPatternBindingMode::Normal,
          }];

          args.push(GenericArgumentPattern::PathPattern(path_segments));
        } else {
          let first_ident: Ident = input.parse()?;
          let mut path_segments = Vec::new();
          path_segments.push(SegmentPattern::Required(SegmentMatcher {
            ident: first_ident,
            args: None,
          }));

          while input.peek(Token![::]) {
            input.parse::<Token![::]>()?;

            if input.peek(Token![$]) {
              input.parse::<Token![$]>()?;

              let (lifetime, mut name) = if input.peek(syn::Lifetime) {
                let lt: syn::Lifetime = input.parse()?;
                (true, lt.ident)
              } else {
                (false, input.parse()?)
              };

              let span = name.span();
              name.set_span(Span::call_site());

              path_segments.push(SegmentPattern::Binding {
                span,
                lifetime,
                name,
                mode: SegmentPatternBindingMode::Normal,
              });
            } else {
              let ident: Ident = input.parse()?;
              let args = if input.peek(Token![<]) {
                Some(input.parse()?)
              } else {
                None
              };

              path_segments.push(SegmentPattern::Required(SegmentMatcher { ident, args }));
            }
          }

          args.push(GenericArgumentPattern::PathPattern(path_segments));
        }
      } else {

        let first_ident: Ident = input.parse()?;

        if input.peek(Token![=]) { // associated type
          input.parse::<Token![=]>()?;

          let value = if input.peek(Token![_]) { // wildcard
            input.parse::<Token![_]>()?;
            AssociatedTypes::Wildcard
          } else if input.peek(Token![$]) { // binding
            input.parse::<Token![$]>()?;

            let (lifetime, mut name) = if input.peek(syn::Lifetime) {
              let lt: syn::Lifetime = input.parse()?;
              (true, lt.ident)
            } else {
              (false, input.parse()?)
            };

            let span = name.span();
            name.set_span(Span::call_site());
            AssociatedTypes::Binding(span, lifetime, name)
          } else {
            let value_ident: Ident = input.parse()?;
            let value_matcher = SegmentMatcher {
              ident: value_ident,
              args: if input.peek(Token![<]) {
                Some(input.parse()?)
              } else {
                None
              },
            };
            AssociatedTypes::Argument(Box::new(value_matcher))
          };

          args.push(GenericArgumentPattern::AssociatedType(
            first_ident.span(),
            first_ident,
            value,
          ));
        } else if input.peek(Token![::]) { // path
          input.parse::<Token![::]>()?;

          let mut path_segments = Vec::new();
          path_segments.push(SegmentPattern::Required(SegmentMatcher {
            ident: first_ident.clone(),
            args: None,
          }));

          loop {
            if input.peek(Token![$]) {
              input.parse::<Token![$]>()?;

              let (lifetime, mut name) = if input.peek(syn::Lifetime) {
                let lt: syn::Lifetime = input.parse()?;
                (true, lt.ident)
              } else {
                (false, input.parse()?)
              };

              let span = name.span();
              name.set_span(Span::call_site());

              path_segments.push(SegmentPattern::Binding {
                span,
                lifetime,
                name,
                mode: SegmentPatternBindingMode::Normal,
              });

              if input.peek(Token![::]) {
                input.parse::<Token![::]>()?;
              } else {
                break;
              }
            } else {
              let ident: Ident = input.parse()?;

              let args = if input.peek(Token![<]) {
                Some(input.parse()?)
              } else {
                None
              };

              path_segments.push(SegmentPattern::Required(SegmentMatcher { ident, args }));

              if input.peek(Token![::]) {
                input.parse::<Token![::]>()?;
              } else {
                break;
              }
            }
          }

          args.push(GenericArgumentPattern::PathPattern(path_segments));
        } else {
          let matcher = SegmentMatcher {
            ident: first_ident,
            args: if input.peek(Token![<]) {
              Some(input.parse()?)
            } else {
              None
            },
          };
          args.push(GenericArgumentPattern::Argument(Box::new(matcher)));
        }
      }

      if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
      } else {
        break;
      }
    }

    input.parse::<Token![>]>()?;
    Ok(ArgumentPattern::AngleBracketed(args))
  }
}

struct PathMatcher {
  path_expr: Expr,
  fully_qualified_check: proc_macro2::TokenStream,
  length_check: proc_macro2::TokenStream,
  segment_checks: Vec<proc_macro2::TokenStream>,
  binding_names: Vec<(Span, bool, Ident, SegmentPatternBindingMode)>,
}

fn generate_path_matcher(
  path_expr: &Expr,
  patterns: &Punctuated<PathPattern, Token![|]>,
) -> Vec<PathMatcher> {
  let mut out = Vec::new();

  for pattern in patterns {
    match pattern {
      PathPattern::Wildcard => {}
      PathPattern::Path {
        segments,
        fully_qualified,
      } => {
        let mut binding_names = Vec::new();
        for seg in segments {
          match seg {
            SegmentPattern::Binding {
              span,
              lifetime,
              name,
              mode,
            } => {
              binding_names.push((span.clone(), *lifetime, name.clone(), mode.clone()));
            }
            SegmentPattern::Required(matcher)
            | SegmentPattern::Optional(matcher) => {
              fn handle_segment_matcher(
                binding_names: &mut Vec<(Span, bool, Ident, SegmentPatternBindingMode)>,
                matcher: &SegmentMatcher,
              ) {
                if let Some(ArgumentPattern::AngleBracketed(args)) =
                  &matcher.args
                {
                  for arg in args {
                    match arg {
                      GenericArgumentPattern::Binding(span, lifetime, name) => {
                        binding_names.push((
                          span.clone(),
                          *lifetime,
                          name.clone(),
                          SegmentPatternBindingMode::Normal,
                        ));
                      }
                      GenericArgumentPattern::Argument(arg) => {
                        handle_segment_matcher(binding_names, &*arg);
                      }
                      GenericArgumentPattern::AssociatedType(_, _, value) => {
                        match value {
                          AssociatedTypes::Binding(span, lifetime, name) => {
                            binding_names.push((
                              span.clone(),
                              *lifetime,
                              name.clone(),
                              SegmentPatternBindingMode::Normal,
                            ));
                          }
                          AssociatedTypes::Argument(arg) => {
                            handle_segment_matcher(binding_names, &*arg);
                          }
                          AssociatedTypes::Wildcard => {}
                        }
                      }
                      GenericArgumentPattern::Wildcard(_) => {}
                      GenericArgumentPattern::PathPattern(path_segments) => {
                        for seg in path_segments {
                          match seg {
                            SegmentPattern::Binding {
                              span,
                              lifetime,
                              name,
                              mode,
                            } => {
                              binding_names.push((
                                span.clone(),
                                *lifetime,
                                name.clone(),
                                mode.clone(),
                              ));
                            }
                            _ => {}
                          }
                        }
                      }
                    }
                  }
                }
              }

              handle_segment_matcher(&mut binding_names, matcher);
            }
          }
        }

        let mut required_segments = Vec::new();
        let mut optional_segments = Vec::new();
        let mut has_multi_binding = false;

        for seg in segments {
          match seg {
            SegmentPattern::Required(matcher) => {
              required_segments.push(matcher)
            }
            SegmentPattern::Optional(matcher) => {
              optional_segments.push(matcher)
            }
            SegmentPattern::Binding { mode, .. } => {
              if matches!(mode, SegmentPatternBindingMode::Multi) {
                has_multi_binding = true;
              }
            }
          }
        }

        let min_len = segments
          .iter()
          .filter(|s| {
            matches!(
              s,
              SegmentPattern::Required(_)
                | SegmentPattern::Binding { mode: SegmentPatternBindingMode::Normal, .. }
            )
          })
          .count();

        let max_len = if has_multi_binding {
          None
        } else {
          Some(segments.len())
        };

        let mut segment_checks = Vec::new();

        for (seg_idx, seg) in segments.iter().enumerate() {
          match seg {
            SegmentPattern::Binding { name, mode, .. } => {
              match mode {
                SegmentPatternBindingMode::Multi => {
                  let required_after = segments[seg_idx + 1..]
                    .iter()
                    .filter(|s| {
                      matches!(
                        s,
                        SegmentPattern::Required(_)
                          | SegmentPattern::Binding { mode: SegmentPatternBindingMode::Normal, .. }
                      )
                    })
                    .count();

                  let check = quote! {
                    let __end_idx = __segments.len() - #required_after;
                    if __idx > __end_idx {
                      break false;
                    }
                    #name = Some(__segments.iter().skip(__idx).take(__end_idx - __idx).cloned().collect::<syn::punctuated::Punctuated<_, syn::Token![::]>>());
                    __idx = __end_idx;
                  };
                  segment_checks.push(check);
                }
                SegmentPatternBindingMode::Normal => {
                  let check = quote! {
                    if __idx >= __segments.len() {
                      break false;
                    }
                    #name = Some(&__segments[__idx]);
                    __idx += 1;
                  };
                  segment_checks.push(check);
                }
                SegmentPatternBindingMode::Optional => {
                  let required_after = segments[seg_idx + 1..]
                    .iter()
                    .filter(|s| {
                      matches!(
                        s,
                        SegmentPattern::Required(_)
                          | SegmentPattern::Binding { mode: SegmentPatternBindingMode::Normal, .. }
                      )
                    })
                    .count();

                  let check = quote! {
                    let __min_needed = __idx + #required_after;
                    if __idx < __segments.len() && __segments.len() > __min_needed {
                      #name = Some(&__segments[__idx]);
                      __idx += 1;
                    } else {
                      #name = None;
                    }
                  };
                  segment_checks.push(check);
                }
              }
              continue;
            }
            _ => {}
          }

          let (matcher, is_optional) = match seg {
            SegmentPattern::Required(m) => (m, false),
            SegmentPattern::Optional(m) => (m, true),
            SegmentPattern::Binding { .. } => unreachable!(),
          };

          fn handle_segment_matcher(
            matcher: &SegmentMatcher,
            is_optional: bool,
          ) -> proc_macro2::TokenStream {
            let seg_ident = &matcher.ident;
            let seg_ident_str = seg_ident.to_string();

            let name_check = quote! {
                __seg.ident == #seg_ident_str
            };

            let args_check = if let Some(args) = &matcher.args {
              match args {
                ArgumentPattern::AngleBracketed(arg_patterns) => {
                  let mut arg_checks = Vec::new();
                  for (arg_idx, arg_pattern) in arg_patterns.iter().enumerate()
                  {
                    match arg_pattern {
                      GenericArgumentPattern::Wildcard(false) => {}
                      GenericArgumentPattern::Wildcard(true) => {
                        arg_checks.push(quote! {
                          if let Some(syn::GenericArgument::Lifetime(_)) = __args.get(#arg_idx) {
                            //
                          } else {
                            break false;
                          }
                        });
                      }
                      GenericArgumentPattern::Argument(segment_matcher) => {
                        fn generate_nested_arg_check(
                          segment_matcher: &SegmentMatcher,
                          arg_var: &str,
                          depth: usize,
                        ) -> proc_macro2::TokenStream {
                          let arg_ident = &segment_matcher.ident;
                          let arg_ident_str = arg_ident.to_string();
                          let arg_var_ident =
                            Ident::new(arg_var, Span::call_site());

                          if let Some(ArgumentPattern::AngleBracketed(
                            nested_arg_patterns,
                          )) = &segment_matcher.args
                          {
                            let mut nested_arg_checks = Vec::new();
                            for (nested_arg_idx, nested_arg_pattern) in
                              nested_arg_patterns.iter().enumerate()
                            {
                              match nested_arg_pattern {
                                GenericArgumentPattern::Binding(
                                  _span,
                                  lifetime,
                                  name,
                                ) => {
                                  let nested_arg_var =
                                    format!("__nested_arg_{}", depth);
                                  let nested_arg_var_ident = Ident::new(
                                    &nested_arg_var,
                                    Span::call_site(),
                                  );
                                  if *lifetime {
                                    nested_arg_checks.push(quote! {
                                      if let Some(syn::GenericArgument::Lifetime(#nested_arg_var_ident)) = __nested_args.get(#nested_arg_idx) {
                                        #name = Some(#nested_arg_var_ident);
                                      } else {
                                        break false;
                                      }
                                    });
                                  } else {
                                    nested_arg_checks.push(quote! {
                                      if let Some(#nested_arg_var_ident) = __nested_args.get(#nested_arg_idx) {
                                        #name = Some(#nested_arg_var_ident);
                                      } else {
                                        break false;
                                      }
                                    });
                                  }
                                }
                                GenericArgumentPattern::Wildcard(_) => {}
                                GenericArgumentPattern::Argument(
                                  inner_segment_matcher,
                                ) => {
                                  let nested_arg_var =
                                    format!("__nested_arg_{}", depth);
                                  let inner_check = generate_nested_arg_check(
                                    inner_segment_matcher,
                                    &nested_arg_var,
                                    depth + 1,
                                  );
                                  let nested_arg_var_ident = Ident::new(
                                    &nested_arg_var,
                                    Span::call_site(),
                                  );
                                  nested_arg_checks.push(quote! {
                                    if let Some(#nested_arg_var_ident) = __nested_args.get(#nested_arg_idx) {
                                      #inner_check
                                    } else {
                                      break false;
                                    }
                                  });
                                }
                                GenericArgumentPattern::AssociatedType(
                                  _span,
                                  ident,
                                  value,
                                ) => {
                                  let nested_arg_var =
                                    format!("__nested_arg_{}", depth);
                                  let nested_arg_var_ident = Ident::new(
                                    &nested_arg_var,
                                    Span::call_site(),
                                  );
                                  let ident_str = ident.to_string();

                                  match value {
                                    AssociatedTypes::Wildcard => {
                                      nested_arg_checks.push(quote! {
                                        if
                                          let Some(#nested_arg_var_ident) = __nested_args.get(#nested_arg_idx)
                                            && let syn::GenericArgument::AssocType(__assoc_type) = #nested_arg_var_ident
                                            && __assoc_type.ident == #ident_str
                                        {
                                          //
                                        } else {
                                          break false;
                                        }
                                      });
                                    }
                                    AssociatedTypes::Binding(
                                      _,
                                      lifetime,
                                      name,
                                    ) => {
                                      if *lifetime {
                                        nested_arg_checks.push(quote! {
                                          if
                                            let Some(#nested_arg_var_ident) = __nested_args.get(#nested_arg_idx)
                                              && let syn::GenericArgument::AssocType(__assoc_type) = #nested_arg_var_ident
                                              && __assoc_type.ident == #ident_str
                                              && let syn::Type::Path(__assoc_path) = &__assoc_type.ty
                                              && let Some(__assoc_seg) = __assoc_path.path.segments.first()
                                              && let syn::PathArguments::AngleBracketed(__assoc_angle_args) = &__assoc_seg.arguments
                                              && let Some(syn::GenericArgument::Lifetime(__lt)) = __assoc_angle_args.args.first()
                                          {
                                            #name = Some(__lt);
                                          } else {
                                            break false;
                                          }
                                        });
                                      } else {
                                        nested_arg_checks.push(quote! {
                                          if
                                            let Some(#nested_arg_var_ident) = __nested_args.get(#nested_arg_idx)
                                              && let syn::GenericArgument::AssocType(__assoc_type) = #nested_arg_var_ident
                                              && __assoc_type.ident == #ident_str
                                          {
                                            #name = Some(&__assoc_type.ty);
                                          } else {
                                            break false;
                                          }
                                        });
                                      }
                                    }
                                    AssociatedTypes::Argument(type_matcher) => {
                                      let type_ident = &type_matcher.ident;
                                      let type_ident_str =
                                        type_ident.to_string();
                                      nested_arg_checks.push(quote! {
                                        if
                                          let Some(#nested_arg_var_ident) = __nested_args.get(#nested_arg_idx)
                                            && let syn::GenericArgument::AssocType(__assoc_type) = #nested_arg_var_ident
                                            && __assoc_type.ident == #ident_str
                                            && let syn::Type::Path(__type_path) = &__assoc_type.ty
                                            && let Some(__last_seg) = __type_path.path.segments.last()
                                            && __last_seg.ident == #type_ident_str
                                        {
                                          //
                                        } else {
                                          break false;
                                        }
                                      });
                                    }
                                  }
                                }
                                GenericArgumentPattern::PathPattern(_) => {
                                  // TODO: Implement path pattern matching in nested generics
                                }
                              }
                            }

                            let nested_arg_count = nested_arg_patterns.len();
                            if arg_ident_str == "__slice__" {
                              let slice_checks = nested_arg_patterns.iter().enumerate().map(|(_idx, pattern)| {
                                match pattern {
                                  GenericArgumentPattern::Binding(_span, _lifetime, name) => {
                                    quote! {
                                      #name = Some(__slice_type.elem.as_ref());
                                    }
                                  }
                                  _ => {
                                    quote!()
                                  }
                                }
                              });
                              quote! {
                                if let syn::GenericArgument::Type(syn::Type::Slice(__slice_type)) = #arg_var_ident {
                                  #(#slice_checks)*
                                } else {
                                  break false;
                                }
                              }
                            } else {
                              quote! {
                                if
                                  let syn::GenericArgument::Type(syn::Type::Path(__nested_type_path)) = #arg_var_ident
                                    && let Some(__nested_seg) = __nested_type_path.path.segments.last()
                                    && __nested_seg.ident == #arg_ident_str
                                    && let syn::PathArguments::AngleBracketed(__nested_angle_args) = &__nested_seg.arguments
                                {
                                  let __nested_args = &__nested_angle_args.args;
                                  if __nested_args.len() != #nested_arg_count {
                                    break false;
                                  }
                                  #(#nested_arg_checks)*
                                } else {
                                  break false;
                                }
                              }
                            }
                          } else {
                            if arg_ident_str == "__slice__" {
                              quote! {
                                if let syn::GenericArgument::Type(syn::Type::Slice(__slice_type)) = #arg_var_ident {
                                  //
                                } else {
                                  break false;
                                }
                              }
                            } else {
                              quote! {
                                if
                                  let syn::GenericArgument::Type(syn::Type::Path(__nested_type_path)) = #arg_var_ident
                                    && let Some(__nested_seg) = __nested_type_path.path.segments.last()
                                    && __nested_seg.ident == #arg_ident_str
                                {
                                  //
                                } else {
                                  break false;
                                }
                              }
                            }
                          }
                        }

                        let nested_check = generate_nested_arg_check(
                          segment_matcher,
                          "__arg",
                          0,
                        );

                        arg_checks.push(quote! {
                          if let Some(__arg) = __args.get(#arg_idx) {
                            #nested_check
                          } else {
                            break false;
                          }
                        });
                      }
                      GenericArgumentPattern::Binding(
                        _span,
                        lifetime,
                        name,
                      ) => {
                        if *lifetime {
                          arg_checks.push(quote! {
                            if let Some(syn::GenericArgument::Lifetime(__arg)) = __args.get(#arg_idx) {
                              #name = Some(__arg);
                            } else {
                              break false;
                            }
                          });
                        } else {
                          arg_checks.push(quote! {
                            if let Some(__arg) = __args.get(#arg_idx) {
                              #name = Some(__arg);
                            } else {
                              break false;
                            }
                          });
                        }
                      }
                      GenericArgumentPattern::AssociatedType(
                        _span,
                        ident,
                        value,
                      ) => {
                        let ident_str = ident.to_string();

                        match value {
                          AssociatedTypes::Wildcard => {
                            arg_checks.push(quote! {
                              if
                                let Some(__arg) = __args.get(#arg_idx)
                                  && let syn::GenericArgument::AssocType(__assoc_type) = __arg
                                  && __assoc_type.ident == #ident_str
                              {
                                //
                              } else {
                                break false;
                              }
                            });
                          }
                          AssociatedTypes::Binding(_, lifetime, name) => {
                            if *lifetime {
                              arg_checks.push(quote! {
                                if
                                  let Some(__arg) = __args.get(#arg_idx)
                                    && let syn::GenericArgument::AssocType(__assoc_type) = __arg
                                    && __assoc_type.ident == #ident_str
                                    && let syn::Type::Path(__assoc_path) = &__assoc_type.ty
                                    && let Some(__assoc_seg) = __assoc_path.path.segments.first()
                                    && let syn::PathArguments::AngleBracketed(__assoc_angle_args) = &__assoc_seg.arguments
                                    && let Some(syn::GenericArgument::Lifetime(__lt)) = __assoc_angle_args.args.first()
                                {
                                  #name = Some(__lt);
                                } else {
                                  break false;
                                }
                              });
                            } else {
                              arg_checks.push(quote! {
                                if
                                  let Some(__arg) = __args.get(#arg_idx)
                                    && let syn::GenericArgument::AssocType(__assoc_type) = __arg
                                    && __assoc_type.ident == #ident_str
                                {
                                  #name = Some(&__assoc_type.ty);
                                } else {
                                  break false;
                                }
                              });
                            }
                          }
                          AssociatedTypes::Argument(type_matcher) => {
                            let type_ident = &type_matcher.ident;
                            let type_ident_str = type_ident.to_string();

                            arg_checks.push(quote! {
                              if
                                let Some(__arg) = __args.get(#arg_idx)
                                  && let syn::GenericArgument::AssocType(__assoc_type) = __arg
                                  && __assoc_type.ident == #ident_str
                                  && let syn::Type::Path(__type_path) = &__assoc_type.ty
                                  && let Some(__last_seg) = __type_path.path.segments.last()
                                  && __last_seg.ident == #type_ident_str
                              {
                                //
                              } else {
                                break false;
                              }
                            });
                          }
                        }
                      }
                      GenericArgumentPattern::PathPattern(path_segments) => {
                        if path_segments.len() == 1 {
                          if let SegmentPattern::Binding { name, .. } = &path_segments[0] {
                            arg_checks.push(quote! {
                              if let Some(syn::GenericArgument::Type(syn::Type::Path(__type_path))) = __args.get(#arg_idx) {
                                #name = Some(&__type_path.path);
                              } else {
                                break false;
                              }
                            });
                          } else {
                            let mut path_checks = Vec::new();
                            for (_seg_idx, seg) in path_segments.iter().enumerate() {
                              match seg {
                                SegmentPattern::Required(matcher) => {
                                  let ident = &matcher.ident;
                                  let ident_str = ident.to_string();
                                  path_checks.push(quote! {
                                    if __path_seg_idx >= __path_segments.len() {
                                      __path_matched = false;
                                      break;
                                    }
                                    if __path_segments[__path_seg_idx].ident != #ident_str {
                                      __path_matched = false;
                                      break;
                                    }
                                    __path_seg_idx += 1;
                                  });
                                }
                                _ => {}
                              }
                            }

                            arg_checks.push(quote! {
                              if let Some(syn::GenericArgument::Type(syn::Type::Path(__type_path))) = __args.get(#arg_idx) {
                                let __path_segments = &__type_path.path.segments;
                                let mut __path_seg_idx = 0;
                                let mut __path_matched = true;

                                loop {
                                  #(#path_checks)*
                                  break;
                                }

                                if !__path_matched || __path_seg_idx != __path_segments.len() {
                                  break false;
                                }
                              } else {
                                break false;
                              }
                            });
                          }
                        } else {
                          let mut path_checks = Vec::new();
                          for (_seg_idx, seg) in path_segments.iter().enumerate() {
                            match seg {
                              SegmentPattern::Required(matcher) => {
                                let ident = &matcher.ident;
                                let ident_str = ident.to_string();
                                path_checks.push(quote! {
                                  if __path_seg_idx >= __path_segments.len() {
                                    __path_matched = false;
                                    break;
                                  }
                                  if __path_segments[__path_seg_idx].ident != #ident_str {
                                    __path_matched = false;
                                    break;
                                  }
                                  __path_seg_idx += 1;
                                });
                              }
                              SegmentPattern::Binding { name, .. } => {
                                path_checks.push(quote! {
                                  if __path_seg_idx >= __path_segments.len() {
                                    __path_matched = false;
                                    break;
                                  }
                                  #name = Some(&__path_segments[__path_seg_idx]);
                                  __path_seg_idx += 1;
                                });
                              }
                              _ => {} // TODO: Handle other segment patterns
                            }
                          }

                          arg_checks.push(quote! {
                            if let Some(syn::GenericArgument::Type(syn::Type::Path(__type_path))) = __args.get(#arg_idx) {
                              let __path_segments = &__type_path.path.segments;
                              let mut __path_seg_idx = 0;
                              let mut __path_matched = true;

                              loop {
                                #(#path_checks)*
                                break;
                              }

                              if !__path_matched || __path_seg_idx != __path_segments.len() {
                                break false;
                              }
                            } else {
                              break false;
                            }
                          });
                        }
                      }
                    }
                  }

                  let arg_count = arg_patterns.len();
                  quote! {
                    if let syn::PathArguments::AngleBracketed(__angle_args) = &__seg.arguments {
                      let __args = &__angle_args.args;
                      if __args.len() != #arg_count {
                        break false;
                      }
                      #(#arg_checks)*
                    } else {
                      break false;
                    }
                  }
                }
              }
            } else {
              quote!()
            };

            let check = if is_optional {
              quote! {
                if __idx < __segments.len() {
                  let __seg = &__segments[__idx];
                  if #name_check {
                    #args_check
                    __idx += 1;
                  }
                }
              }
            } else {
              quote! {
                if __idx >= __segments.len() {
                  break false;
                }
                let __seg = &__segments[__idx];
                if !(#name_check) {
                  break false;
                }
                #args_check
                __idx += 1;
              }
            };

            check
          }

          segment_checks.push(handle_segment_matcher(matcher, is_optional));
        }

        let fully_qualified_check = if *fully_qualified {
          quote! {
            if !__path.leading_colon.is_some() {
              __matched = false;
            }
          }
        } else {
          quote!()
        };

        let length_check = match max_len {
          Some(max) if min_len == max => {
            quote! {
              if __segments.len() != #min_len {
                __matched = false;
              }
            }
          }
          Some(max) => {
            quote! {
              if __segments.len() < #min_len || __segments.len() > #max {
                __matched = false;
              }
            }
          }
          None => {
            quote! {
              if __segments.len() < #min_len {
                __matched = false;
              }
            }
          }
        };

        out.push(PathMatcher {
          path_expr: path_expr.clone(),
          fully_qualified_check,
          length_check,
          segment_checks,
          binding_names,
        })
      }
    }
  }

  out
}

#[proc_macro]
pub fn path_match(input: TokenStream) -> TokenStream {
  let PathMatchArms { path, arms } = parse_macro_input!(input as PathMatchArms);

  let wildcard_arm = arms.last().filter(|arm| {
    arm
      .patterns
      .first()
      .is_some_and(|pattern| matches!(pattern, PathPattern::Wildcard))
  });

  if wildcard_arm.is_none() {
    return syn::Error::new(
      Span::call_site(),
      "path_match! requires a wildcard arm `_ => ...` as the last arm",
    )
    .to_compile_error()
    .into();
  }

  let wildcard_body = &wildcard_arm.unwrap().body;
  let non_wildcard_arms = &arms[..arms.len() - 1];

  for arm in non_wildcard_arms {
    if arm
      .patterns
      .iter()
      .any(|pattern| matches!(pattern, PathPattern::Wildcard))
    {
      return syn::Error::new(
        Span::call_site(),
        "wildcard pattern `_` must be the last arm",
      )
      .to_compile_error()
      .into();
    }
  }

  let mut match_checks = Vec::new();

  for arm in non_wildcard_arms {
    let path_matchers = generate_path_matcher(&path, &arm.patterns);
    for PathMatcher {
      path_expr,
      fully_qualified_check,
      length_check,
      segment_checks,
      binding_names,
    } in path_matchers
    {
      match_checks.push((
        path_expr,
        fully_qualified_check,
        length_check,
        segment_checks,
        binding_names,
        &arm.body,
      ));
    }
  }

  let arms_code = match_checks.into_iter().map(
    |(path_expr, fq_check, len_check, seg_checks, binding_names, body)| {
      let spanless_binding_names = binding_names
        .iter()
        .map(|(_, _, name, _)| name.clone())
        .collect::<Vec<_>>();
      let binding_extractions =
        binding_names.into_iter().map(|(span, _lifetime, name, mode)| {
          let mut name_in_some = name.clone();
          name_in_some.set_span(span);

          match mode {
            SegmentPatternBindingMode::Optional => {
              quote! {
                let #name_in_some = #name;
              }
            }
            _ => {
              quote! {
                let #name_in_some = #name.unwrap();
              }
            }
          }
        });

      quote! {
        {
          let __path = #path_expr;
          let __segments = &__path.segments;
          let mut __idx = 0;
          let mut __matched = true;

          #(let mut #spanless_binding_names: Option<_> = None;)*

          #fq_check

          if __matched {
            #len_check
          }

          if __matched {
            __matched = loop {
              #(#seg_checks)*
              break __matched;
            }
          }

          if __matched && __idx != __segments.len() {
            __matched = false;
          }

          if __matched {
            #(#binding_extractions)*
            return #body;
          }
        }
      }
    },
  );

  let expanded = quote! {
    (|| {
      #(#arms_code)*

      #wildcard_body
    })()
  };

  TokenStream::from(expanded)
}
