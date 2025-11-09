use syn_match::path_match;

#[test]
fn test_simple_path() {
  let path: syn::Path = syn::parse_quote!(String);
  let result = path_match!(&path, String => "matched", _ => "no match");
  assert_eq!(result, "matched");
}

#[test]
fn test_fully_qualified() {
  let path: syn::Path = syn::parse_quote!(::std::string::String);
  let result =
    path_match!(&path, ::std::string::String => "matched", _ => "no match");
  assert_eq!(result, "matched");
}

#[test]
fn test_optional_segments() {
  let path1: syn::Path = syn::parse_quote!(std::str::String);
  let path2: syn::Path = syn::parse_quote!(str::String);
  let path3: syn::Path = syn::parse_quote!(String);

  let r1 = path_match!(&path1, std?::str?::String => "matched", _ => "no");
  let r2 = path_match!(&path2, std?::str?::String => "matched", _ => "no");
  let r3 = path_match!(&path3, std?::str?::String => "matched", _ => "no");

  assert_eq!(r1, "matched");
  assert_eq!(r2, "matched");
  assert_eq!(r3, "matched");
}

#[test]
fn test_single_binding() {
  let path: syn::Path = syn::parse_quote!(std::str::String);
  let result = path_match!(
    &path,
    std::str::String => "String".to_string(),
    std::str::$rest+ => {
      rest.iter().map(|seg| seg.ident.to_string()).collect::<String>()
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "String");
}

#[test]
fn test_multiple_bindings() {
  let path: syn::Path = syn::parse_quote!(std::collections::HashMap);
  let result = path_match!(
    &path,
    std::$module::$ty => format!("{}::{}", module.ident, ty.ident),
    _ => "no match".to_string(),
  );
  assert_eq!(result, "collections::HashMap");
}

#[test]
fn test_wildcard_fallback() {
  let path: syn::Path = syn::parse_quote!(Something::Random);
  let result = path_match!(
    &path,
    String => "string",
    _ => "wildcard",
  );
  assert_eq!(result, "wildcard");
}

#[test]
fn test_multi_patterns() {
  let path: syn::Path = syn::parse_quote!(String);
  let path2: syn::Path = syn::parse_quote!(HashMap);

  let result = path_match!(
    &path,
    String | HashMap => "correct",
    _ => "wildcard",
  );
  let result2 = path_match!(
    &path2,
    String | HashMap => "correct",
    _ => "wildcard",
  );
  assert_eq!(result, "correct");
  assert_eq!(result2, "correct");
}

#[test]
fn test_multi_binding_at_start() {
  let path: syn::Path = syn::parse_quote!(std::collections::HashMap);

  let result = path_match!(
    &path,
    $prefix+::HashMap => {
      prefix.iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "std::collections");
}

#[test]
fn test_multi_binding_at_start_single_segment() {
  let path: syn::Path = syn::parse_quote!(std::String);

  let result = path_match!(
    &path,
    $prefix+::String => {
      prefix.iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "std");
}

#[test]
fn test_multi_binding_at_start_multiple_after() {
  let path: syn::Path = syn::parse_quote!(std::collections::hash::HashMap);

  let result = path_match!(
    &path,
    $prefix+::hash::HashMap => {
      prefix.iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "std::collections");
}

#[test]
fn test_generic_binding() {
  let path: syn::Path = syn::parse_quote!(Option<String>);

  let result = path_match!(
    &path,
    Option<$inner> => {
      if let syn::GenericArgument::Type(syn::Type::Path(type_path)) = inner {
        type_path.path.segments.last().unwrap().ident.to_string()
      } else {
        "not a path".to_string()
      }
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "String");
}

#[test]
fn test_generic_binding_nested() {
  let path: syn::Path =
    syn::parse_quote!(Option<Outer<Other<More<Yet<String>>>>>);

  let result = path_match!(
    &path,
    Option<Outer<Other<More<Yet<$inner>>>>> => {
      if let syn::GenericArgument::Type(syn::Type::Path(type_path)) = inner {
        type_path.path.segments.last().unwrap().ident.to_string()
      } else {
        "not a path".to_string()
      }
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "String");
}

#[test]
fn test_result_generic_bindings() {
  let path: syn::Path = syn::parse_quote!(Result<String, Error>);

  let result = path_match!(
    &path,
    Result<$ok, $err> => {
      format!("Result<{}, {}>",
        if let syn::GenericArgument::Type(syn::Type::Path(p)) = ok { p.path.segments.last().unwrap().ident.to_string() } else { "?".to_string() },
        if let syn::GenericArgument::Type(syn::Type::Path(p)) = err { p.path.segments.last().unwrap().ident.to_string() } else { "?".to_string() }
      )
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "Result<String, Error>");
}

#[test]
fn test_type_assignment() {
  let path: syn::Path = syn::parse_quote!(Future<Output = String>);
  
  let result = path_match!(
    &path,
    Future<Output = $output> => {
      if let syn::Type::Path(type_path) = output {
        format!("Future output: {}", type_path.path.segments.last().unwrap().ident.to_string())
      } else {
        "not a path".to_string()
      }
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "Future output: String");
}

#[test]
fn test_type_assignment_mismatch() {
  let path: syn::Path = syn::parse_quote!(Future<Input = String>);

  let result = path_match!(
    &path,
    Future<Output = $output> => {
      if let syn::Type::Path(type_path) = output {
        format!("Future output: {}", type_path.path.segments.last().unwrap().ident.to_string())
      } else {
        "not a path".to_string()
      }
    },
    _ => "no match".to_string(),
  );

  assert_eq!(result, "no match");
}

#[test]
fn test_type_assignment_wildcard() {
  let path: syn::Path = syn::parse_quote!(Future<Output = String>);

  let result = path_match!(
    &path,
    Future<Output = _> => "matched with wildcard",
    _ => "no match",
  );

  assert_eq!(result, "matched with wildcard");
}

#[test]
fn test_type_assignment_specific_type() {
  let path: syn::Path = syn::parse_quote!(Future<Output = String>);

  let result = path_match!(
    &path,
    Future<Output = String> => "matched specific type",
    Future<Output = i32> => "matched i32",
    _ => "no match",
  );

  assert_eq!(result, "matched specific type");
}
