/// Metadata and default highlight query for a built-in Tree-sitter language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrammarSpec {
    pub canonical_id: &'static str,
    pub cdn_path: &'static str,
    pub default_highlights_scm: &'static str,
}

impl GrammarSpec {
    #[inline]
    pub fn wasm_symbol(&self) -> &'static str {
        self.canonical_id
    }
}

use std::borrow::Cow;

pub(crate) fn extract_fence_token(lang_hint: &str) -> Option<Cow<'_, str>> {
    let raw = crate::markdown::prefix::fence_info(lang_hint).unwrap_or(lang_hint.trim());
    let token = raw
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{' || c == ';')
        .next()
        .unwrap_or("")
        .trim();

    if token.is_empty() {
        return None;
    }

    if token.bytes().any(|b| b.is_ascii_uppercase()) {
        Some(Cow::Owned(token.to_ascii_lowercase()))
    } else {
        Some(Cow::Borrowed(token))
    }
}

/// Maps a known language alias to its canonical language identifier.
pub fn canonical_language_id(token: &str) -> Option<&'static str> {
    match token {
        // Built-in languages:
        "rust" | "rs" => Some("rust"),
        "json" | "jsonc" | "jsonl" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "python" | "py" | "py3" => Some("python"),
        "javascript" | "js" | "mjs" | "cjs" | "jsx" => Some("javascript"),
        "typescript" | "ts" | "mts" | "cts" => Some("typescript"),
        "tsx" => Some("tsx"),
        "bash" | "sh" | "shell" | "zsh" | "fish" => Some("bash"),
        "c" | "h" => Some("c"),
        "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hxx" => Some("cpp"),
        "csharp" | "cs" | "c#" | "c_sharp" => Some("c_sharp"),
        "go" | "golang" => Some("go"),
        "java" => Some("java"),
        "kotlin" | "kt" | "kts" => Some("kotlin"),
        "ruby" | "rb" => Some("ruby"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        "zig" => Some("zig"),
        "lua" => Some("lua"),
        "html" | "htm" | "xml" | "svg" => Some("html"),
        "css" | "scss" => Some("css"),

        // Remote / niche language aliases and short-token exemptions:
        "haskell" | "hs" => Some("haskell"),
        "elixir" | "ex" | "exs" => Some("elixir"),
        "erlang" | "erl" | "hrl" => Some("erlang"),
        "ocaml" | "ml" | "mli" => Some("ocaml"),
        "julia" | "jl" => Some("julia"),
        "clojure" | "clj" | "cljs" | "cljc" | "edn" => Some("clojure"),
        "hcl" | "tf" | "terraform" => Some("hcl"),
        "make" | "makefile" | "mk" | "gnumakefile" => Some("make"),
        "dockerfile" | "docker" => Some("dockerfile"),
        "graphql" | "gql" => Some("graphql"),
        "proto" | "protobuf" => Some("proto"),
        "solidity" | "sol" => Some("solidity"),
        "typst" | "typ" => Some("typst"),
        "glsl" | "frag" | "vert" | "comp" => Some("glsl"),
        "perl" | "pl" => Some("perl"),
        "diff" | "patch" => Some("diff"),

        _ => None,
    }
}

/// Returns the built-in [`GrammarSpec`] for `canonical_id`, if it is a built-in grammar.
pub fn builtin_spec_for_canonical_id(canonical_id: &str) -> Option<GrammarSpec> {
    match canonical_id {
        "rust" => Some(GrammarSpec {
            canonical_id: "rust",
            cdn_path: "tree-sitter-rust@latest/tree-sitter-rust.wasm",
            default_highlights_scm: RUST_HIGHLIGHTS_SCM,
        }),
        "json" => Some(GrammarSpec {
            canonical_id: "json",
            cdn_path: "tree-sitter-json@latest/tree-sitter-json.wasm",
            default_highlights_scm: JSON_HIGHLIGHTS_SCM,
        }),
        "toml" => Some(GrammarSpec {
            canonical_id: "toml",
            cdn_path: "@tree-sitter-grammars/tree-sitter-toml@latest/tree-sitter-toml.wasm",
            default_highlights_scm: TOML_HIGHLIGHTS_SCM,
        }),
        "yaml" => Some(GrammarSpec {
            canonical_id: "yaml",
            cdn_path: "@tree-sitter-grammars/tree-sitter-yaml@latest/tree-sitter-yaml.wasm",
            default_highlights_scm: YAML_HIGHLIGHTS_SCM,
        }),
        "python" => Some(GrammarSpec {
            canonical_id: "python",
            cdn_path: "tree-sitter-python@latest/tree-sitter-python.wasm",
            default_highlights_scm: PYTHON_HIGHLIGHTS_SCM,
        }),
        "javascript" => Some(GrammarSpec {
            canonical_id: "javascript",
            cdn_path: "tree-sitter-javascript@latest/tree-sitter-javascript.wasm",
            default_highlights_scm: JAVASCRIPT_HIGHLIGHTS_SCM,
        }),
        "typescript" => Some(GrammarSpec {
            canonical_id: "typescript",
            cdn_path: "tree-sitter-typescript@latest/tree-sitter-typescript.wasm",
            default_highlights_scm: TYPESCRIPT_HIGHLIGHTS_SCM,
        }),
        "tsx" => Some(GrammarSpec {
            canonical_id: "tsx",
            cdn_path: "tree-sitter-typescript@latest/tree-sitter-tsx.wasm",
            default_highlights_scm: TYPESCRIPT_HIGHLIGHTS_SCM,
        }),
        "bash" => Some(GrammarSpec {
            canonical_id: "bash",
            cdn_path: "tree-sitter-bash@latest/tree-sitter-bash.wasm",
            default_highlights_scm: BASH_HIGHLIGHTS_SCM,
        }),
        "c" => Some(GrammarSpec {
            canonical_id: "c",
            cdn_path: "tree-sitter-c@latest/tree-sitter-c.wasm",
            default_highlights_scm: C_HIGHLIGHTS_SCM,
        }),
        "cpp" => Some(GrammarSpec {
            canonical_id: "cpp",
            cdn_path: "tree-sitter-cpp@latest/tree-sitter-cpp.wasm",
            default_highlights_scm: CPP_HIGHLIGHTS_SCM,
        }),
        "c_sharp" => Some(GrammarSpec {
            canonical_id: "c_sharp",
            cdn_path: "tree-sitter-c-sharp@latest/tree-sitter-c_sharp.wasm",
            default_highlights_scm: CSHARP_HIGHLIGHTS_SCM,
        }),
        "go" => Some(GrammarSpec {
            canonical_id: "go",
            cdn_path: "tree-sitter-go@latest/tree-sitter-go.wasm",
            default_highlights_scm: GO_HIGHLIGHTS_SCM,
        }),
        "java" => Some(GrammarSpec {
            canonical_id: "java",
            cdn_path: "tree-sitter-java@latest/tree-sitter-java.wasm",
            default_highlights_scm: JAVA_HIGHLIGHTS_SCM,
        }),
        "kotlin" => Some(GrammarSpec {
            canonical_id: "kotlin",
            cdn_path: "@tree-sitter-grammars/tree-sitter-kotlin@latest/tree-sitter-kotlin.wasm",
            default_highlights_scm: KOTLIN_HIGHLIGHTS_SCM,
        }),
        "ruby" => Some(GrammarSpec {
            canonical_id: "ruby",
            cdn_path: "tree-sitter-ruby@latest/tree-sitter-ruby.wasm",
            default_highlights_scm: RUBY_HIGHLIGHTS_SCM,
        }),
        "php" => Some(GrammarSpec {
            canonical_id: "php",
            cdn_path: "tree-sitter-php@latest/tree-sitter-php.wasm",
            default_highlights_scm: PHP_HIGHLIGHTS_SCM,
        }),
        "swift" => Some(GrammarSpec {
            canonical_id: "swift",
            cdn_path: "@binclusive/tree-sitter-swift-wasm@latest/wasm/tree-sitter-swift.wasm",
            default_highlights_scm: SWIFT_HIGHLIGHTS_SCM,
        }),
        "zig" => Some(GrammarSpec {
            canonical_id: "zig",
            cdn_path: "@tree-sitter-grammars/tree-sitter-zig@latest/tree-sitter-zig.wasm",
            default_highlights_scm: ZIG_HIGHLIGHTS_SCM,
        }),
        "lua" => Some(GrammarSpec {
            canonical_id: "lua",
            cdn_path: "@tree-sitter-grammars/tree-sitter-lua@latest/tree-sitter-lua.wasm",
            default_highlights_scm: LUA_HIGHLIGHTS_SCM,
        }),
        "html" => Some(GrammarSpec {
            canonical_id: "html",
            cdn_path: "tree-sitter-html@latest/tree-sitter-html.wasm",
            default_highlights_scm: HTML_HIGHLIGHTS_SCM,
        }),
        "css" => Some(GrammarSpec {
            canonical_id: "css",
            cdn_path: "tree-sitter-css@latest/tree-sitter-css.wasm",
            default_highlights_scm: CSS_HIGHLIGHTS_SCM,
        }),
        _ => None,
    }
}

/// Resolves a Markdown code fence info-string (such as `"rs"`, `"python,ignore"`, `"c++"`)
/// into a built-in [`GrammarSpec`].
pub fn resolve_grammar_spec(lang_hint: &str) -> Option<GrammarSpec> {
    let token = extract_fence_token(lang_hint)?;
    let canonical = canonical_language_id(token.as_ref())?;
    builtin_spec_for_canonical_id(canonical)
}

pub const JSON_HIGHLIGHTS_SCM: &str = r#"
(string) @string
(pair key: (string) @property)
(escape_sequence) @string.escape
(number) @number
(true) @boolean
(false) @boolean
(null) @constant.builtin
(comment) @comment
["{" "}" "[" "]"] @punctuation.bracket
[":" ","] @punctuation.delimiter
"#;

pub const TOML_HIGHLIGHTS_SCM: &str = r#"
(bare_key) @property
(quoted_key) @property
(string) @string
(escape_sequence) @string.escape
(integer) @number
(float) @number
(boolean) @boolean
(local_date) @number
(local_time) @number
(local_date_time) @number
(offset_date_time) @number
(comment) @comment
["[" "]" "[[" "]]" "{" "}"] @punctuation.bracket
["." "," "="] @punctuation.delimiter
"#;

pub const YAML_HIGHLIGHTS_SCM: &str = r#"
(block_mapping_pair key: (flow_node) @property)
(flow_mapping (_ key: (flow_node) @property))
(double_quote_scalar) @string
(single_quote_scalar) @string
(block_scalar) @string
(escape_sequence) @string.escape
(integer_scalar) @number
(float_scalar) @number
(boolean_scalar) @boolean
(null_scalar) @constant.builtin
(anchor_name) @attribute
(alias_name) @attribute
(tag) @type
(comment) @comment
["[" "]" "{" "}"] @punctuation.bracket
[":" "," "-" "?" "|"] @punctuation.delimiter
"#;

pub const RUST_HIGHLIGHTS_SCM: &str = r#"
(type_identifier) @type
(primitive_type) @type.builtin
(field_identifier) @property
(shorthand_field_initializer (identifier) @property)
(function_item name: (identifier) @function)
(function_signature_item name: (identifier) @function)
(call_expression function: (identifier) @function)
(call_expression function: (field_expression field: (field_identifier) @function.method))
(call_expression function: (scoped_identifier name: (identifier) @function))
(macro_invocation macro: (identifier) @attribute)
(macro_definition name: (identifier) @attribute)
(attribute_item) @attribute
(inner_attribute_item) @attribute
(lifetime) @attribute
(line_comment) @comment
(block_comment) @comment
(string_literal) @string
(raw_string_literal) @string
(char_literal) @string
(escape_sequence) @string.escape
(integer_literal) @number
(float_literal) @number
(boolean_literal) @boolean
(self) @variable.builtin
(crate) @keyword
(super) @keyword
(mutable_specifier) @keyword
[
  "as" "async" "await" "break" "const" "continue" "dyn" "else"
  "enum" "extern" "fn" "for" "if" "impl" "in" "let" "loop" "match"
  "mod" "move" "pub" "ref" "return" "static" "struct" "trait"
  "type" "union" "unsafe" "use" "where" "while"
] @keyword
[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket
[
  "::" ":" "." "," ";"
] @punctuation.delimiter
[
  "!" "!=" "%" "%=" "&" "&&" "&=" "*" "*=" "+" "+=" "-" "-=" "->" "=>"
  ".." "..=" "/" "/=" "<" "<<" "<<=" "<=" "=" "==" ">" ">=" ">>" ">>="
  "?" "@" "^" "^=" "|" "|=" "||"
] @operator
"#;

pub const PYTHON_HIGHLIGHTS_SCM: &str = r#"
(attribute attribute: (identifier) @property)
(function_definition name: (identifier) @function)
(class_definition name: (identifier) @type)
(call function: (identifier) @function)
(call function: (attribute attribute: (identifier) @function.method))
(decorator) @attribute
(comment) @comment
(string) @string
(escape_sequence) @string.escape
(integer) @number
(float) @number
(true) @boolean
(false) @boolean
(none) @constant.builtin
[
  "and" "as" "assert" "async" "await" "break" "class" "continue" "def"
  "del" "elif" "else" "except" "finally" "for" "from" "global" "if"
  "import" "in" "is" "lambda" "nonlocal" "not" "or" "pass" "raise"
  "return" "try" "while" "with" "yield" "match" "case"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," "." ":" ";"] @punctuation.delimiter
["+" "-" "*" "**" "/" "//" "%" "@" "==" "!=" "<" ">" "<=" ">=" "=" "+=" "-=" "*=" "/=" "->"] @operator
"#;

pub const JAVASCRIPT_HIGHLIGHTS_SCM: &str = r#"
(property_identifier) @property
(function_declaration name: (identifier) @function)
(function_expression name: (identifier) @function)
(variable_declarator name: (identifier) @function value: [(function_expression) (arrow_function)])
(assignment_expression left: (identifier) @function right: [(function_expression) (arrow_function)])
(assignment_expression left: (member_expression property: (property_identifier) @function.method) right: [(function_expression) (arrow_function)])
(method_definition name: (property_identifier) @function.method)
(call_expression function: (identifier) @function)
(call_expression function: (member_expression property: (property_identifier) @function.method))
(class_declaration name: (identifier) @type)
(decorator) @attribute
(comment) @comment
(string) @string
(template_string) @string
(regex) @string.regex
(escape_sequence) @string.escape
(number) @number
(true) @boolean
(false) @boolean
(null) @constant.builtin
(undefined) @constant.builtin
(this) @variable.builtin
(super) @variable.builtin
[
  "async" "await" "break" "case" "catch" "class" "const" "continue"
  "debugger" "default" "delete" "do" "else" "export" "extends" "finally"
  "for" "from" "function" "get" "if" "import" "in" "instanceof" "let"
  "new" "of" "return" "set" "static" "switch" "throw" "try" "typeof"
  "var" "void" "while" "with" "yield"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," "." ";" ":"] @punctuation.delimiter
["+" "-" "*" "/" "%" "==" "===" "!=" "!==" "<" ">" "<=" ">=" "=" "=>" "&&" "||" "!" "?" "..."] @operator
"#;

pub const TYPESCRIPT_HIGHLIGHTS_SCM: &str = r#"
(type_identifier) @type
(predefined_type) @type.builtin
(property_identifier) @property
(function_declaration name: (identifier) @function)
(function_expression name: (identifier) @function)
(variable_declarator name: (identifier) @function value: [(function_expression) (arrow_function)])
(assignment_expression left: (identifier) @function right: [(function_expression) (arrow_function)])
(assignment_expression left: (member_expression property: (property_identifier) @function.method) right: [(function_expression) (arrow_function)])
(method_definition name: (property_identifier) @function.method)
(call_expression function: (identifier) @function)
(call_expression function: (member_expression property: (property_identifier) @function.method))
(class_declaration name: (type_identifier) @type)
(interface_declaration name: (type_identifier) @type)
(type_alias_declaration name: (type_identifier) @type)
(decorator) @attribute
(comment) @comment
(string) @string
(template_string) @string
(regex) @string.regex
(escape_sequence) @string.escape
(number) @number
(true) @boolean
(false) @boolean
(null) @constant.builtin
(undefined) @constant.builtin
(this) @variable.builtin
(super) @variable.builtin
[
  "abstract" "as" "async" "await" "break" "case" "catch" "class" "const"
  "continue" "debugger" "declare" "default" "delete" "do" "else" "enum"
  "export" "extends" "finally" "for" "from" "function" "get" "if"
  "implements" "import" "in" "infer" "instanceof" "interface" "is"
  "keyof" "let" "module" "namespace" "new" "of" "override" "private"
  "protected" "public" "readonly" "return" "satisfies" "set" "static"
  "switch" "throw" "try" "type" "typeof" "var" "void" "while" "with" "yield"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," "." ";" ":"] @punctuation.delimiter
["+" "-" "*" "/" "%" "==" "===" "!=" "!==" "<" ">" "<=" ">=" "=" "=>" "&&" "||" "!" "?" "..."] @operator
"#;

pub const BASH_HIGHLIGHTS_SCM: &str = r#"
(function_definition name: (word) @function)
(command_name (word) @function)
(variable_name) @variable
(string) @string
(raw_string) @string
(ansi_c_string) @string
(heredoc_body) @string
(number) @number
(comment) @comment
(file_descriptor) @number
[
  "case" "do" "done" "elif" "else" "esac" "export" "fi" "for" "function"
  "if" "in" "local" "readonly" "select" "then" "unset" "until" "while"
] @keyword
["(" ")" "[" "]" "[[" "]]" "{" "}" "$("] @punctuation.bracket
[";" ";;" "&" "|" "&&" "||" ">" ">>" "<" "="] @operator
"#;

pub const C_HIGHLIGHTS_SCM: &str = r#"
(type_identifier) @type
(primitive_type) @type.builtin
(sized_type_specifier) @type.builtin
(field_identifier) @property
(function_declarator declarator: (identifier) @function)
(call_expression function: (identifier) @function)
(call_expression function: (field_expression field: (field_identifier) @function.method))
(preproc_include) @keyword
(preproc_def) @attribute
(preproc_function_def) @attribute
(preproc_ifdef) @attribute
(comment) @comment
(string_literal) @string
(system_lib_string) @string
(char_literal) @string
(escape_sequence) @string.escape
(number_literal) @number
(true) @boolean
(false) @boolean
(null) @constant.builtin
[
  "auto" "break" "case" "const" "continue" "default" "do" "else" "enum"
  "extern" "for" "goto" "if" "inline" "register" "restrict" "return"
  "sizeof" "static" "struct" "switch" "typedef" "union" "volatile" "while"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," "." ";" ":" "->"] @punctuation.delimiter
["+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">=" "=" "&&" "||" "!" "&" "|" "^" "~" "<<" ">>"] @operator
"#;

pub const CPP_HIGHLIGHTS_SCM: &str = r#"
(type_identifier) @type
(primitive_type) @type.builtin
(sized_type_specifier) @type.builtin
(namespace_identifier) @type
(field_identifier) @property
(function_declarator declarator: (identifier) @function)
(function_declarator declarator: (field_identifier) @function.method)
(function_declarator declarator: (qualified_identifier name: (identifier) @function))
(call_expression function: (identifier) @function)
(call_expression function: (field_expression field: (field_identifier) @function.method))
(call_expression function: (qualified_identifier name: (identifier) @function))
(preproc_include) @keyword
(preproc_def) @attribute
(comment) @comment
(string_literal) @string
(raw_string_literal) @string
(system_lib_string) @string
(char_literal) @string
(escape_sequence) @string.escape
(number_literal) @number
(true) @boolean
(false) @boolean
(null) @constant.builtin
(this) @variable.builtin
(auto) @keyword
[
  "alignas" "alignof" "break" "case" "catch" "class" "concept"
  "const" "consteval" "constexpr" "constinit" "continue" "co_await"
  "co_return" "co_yield" "decltype" "default" "delete" "do" "else"
  "enum" "explicit" "extern" "final" "for" "friend" "goto"
  "if" "inline" "mutable" "namespace" "new" "noexcept" "operator"
  "override" "private" "protected" "public" "register" "requires"
  "return" "sizeof" "static" "static_assert" "struct" "switch"
  "template" "throw" "try" "typedef" "typename" "union"
  "using" "virtual" "volatile" "while"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," "." ";" ":" "::" "->"] @punctuation.delimiter
["+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">=" "=" "&&" "||" "!" "&" "|" "^" "~" "<<" ">>"] @operator
"#;

pub const CSHARP_HIGHLIGHTS_SCM: &str = r#"
(method_declaration name: (identifier) @function.method)
(local_function_statement name: (identifier) @function)
(class_declaration name: (identifier) @type)
(interface_declaration name: (identifier) @type)
(struct_declaration name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(record_declaration name: (identifier) @type)
(predefined_type) @type.builtin
(attribute) @attribute
(comment) @comment
(string_literal) @string
(verbatim_string_literal) @string
(interpolated_string_expression) @string
(character_literal) @string
(integer_literal) @number
(real_literal) @number
(boolean_literal) @boolean
(null_literal) @constant.builtin
[
  "abstract" "as" "async" "await" "base" "break" "case" "catch"
  "checked" "class" "const" "continue" "default" "delegate" "do" "else"
  "enum" "event" "explicit" "extern" "finally" "fixed" "for" "foreach"
  "goto" "if" "implicit" "in" "interface" "internal" "is" "lock"
  "namespace" "new" "operator" "out" "override" "params" "private"
  "protected" "public" "readonly" "record" "ref" "return" "sealed"
  "sizeof" "stackalloc" "static" "struct" "switch" "this" "throw" "try"
  "typeof" "unchecked" "unsafe" "using" "var" "virtual" "volatile" "while" "yield"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," "." ";" ":"] @punctuation.delimiter
"#;

pub const GO_HIGHLIGHTS_SCM: &str = r#"
(type_identifier) @type
(field_identifier) @property
(package_identifier) @type
(function_declaration name: (identifier) @function)
(method_declaration name: (field_identifier) @function.method)
(call_expression function: (identifier) @function)
(call_expression function: (selector_expression field: (field_identifier) @function.method))
(comment) @comment
(interpreted_string_literal) @string
(raw_string_literal) @string
(rune_literal) @string
(escape_sequence) @string.escape
(int_literal) @number
(float_literal) @number
(imaginary_literal) @number
(true) @boolean
(false) @boolean
(nil) @constant.builtin
(iota) @constant.builtin
[
  "break" "case" "chan" "const" "continue" "default" "defer" "else"
  "fallthrough" "for" "func" "go" "goto" "if" "import" "interface"
  "map" "package" "range" "return" "select" "struct" "switch" "type" "var"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," "." ";" ":"] @punctuation.delimiter
["+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">=" "=" ":=" "<-" "&&" "||" "!" "&" "|" "^" "<<" ">>"] @operator
"#;

pub const JAVA_HIGHLIGHTS_SCM: &str = r#"
(type_identifier) @type
(integral_type) @type.builtin
(floating_point_type) @type.builtin
(boolean_type) @type.builtin
(void_type) @type.builtin
(method_declaration name: (identifier) @function.method)
(method_invocation name: (identifier) @function.method)
(class_declaration name: (identifier) @type)
(interface_declaration name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(annotation) @attribute
(marker_annotation) @attribute
(line_comment) @comment
(block_comment) @comment
(string_literal) @string
(character_literal) @string
(decimal_integer_literal) @number
(hex_integer_literal) @number
(decimal_floating_point_literal) @number
(true) @boolean
(false) @boolean
(null_literal) @constant.builtin
(this) @variable.builtin
(super) @variable.builtin
[
  "abstract" "assert" "break" "case" "catch" "class" "continue" "default"
  "do" "else" "enum" "extends" "final" "finally" "for" "if" "implements"
  "import" "instanceof" "interface" "native" "new" "package" "private"
  "protected" "public" "record" "return" "static" "strictfp" "switch"
  "synchronized" "throw" "throws" "transient" "try" "volatile" "while"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," "." ";" ":"] @punctuation.delimiter
"#;

pub const KOTLIN_HIGHLIGHTS_SCM: &str = r#"
(user_type (identifier) @type)
(function_declaration (identifier) @function)
(class_declaration (identifier) @type)
(call_expression (identifier) @function)
(annotation) @attribute
(line_comment) @comment
(block_comment) @comment
(string_literal) @string
(multiline_string_literal) @string
(character_literal) @string
(number_literal) @number
(float_literal) @number
(this_expression) @variable.builtin
(super_expression) @variable.builtin
(reification_modifier) @keyword
[
  "abstract" "actual" "annotation" "as" "by" "catch" "class"
  "companion" "const" "constructor" "crossinline" "data" "do"
  "else" "enum" "expect" "external" "final" "finally" "for" "fun" "if"
  "import" "in" "infix" "init" "inline" "inner" "interface" "internal"
  "is" "lateinit" "noinline" "object" "open" "operator" "out" "override"
  "package" "private" "protected" "public" "return" "sealed"
  "super" "suspend" "tailrec" "this" "throw" "try" "typealias" "val"
  "var" "vararg" "when" "where" "while"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
"#;

pub const RUBY_HIGHLIGHTS_SCM: &str = r#"
(method name: (identifier) @function.method)
(singleton_method name: (identifier) @function.method)
(call method: (identifier) @function.method)
(class name: (constant) @type)
(module name: (constant) @type)
(constant) @type
(instance_variable) @property
(class_variable) @property
(global_variable) @variable
(simple_symbol) @string
(string) @string
(integer) @number
(float) @number
(true) @boolean
(false) @boolean
(nil) @constant.builtin
(self) @variable.builtin
(super) @variable.builtin
(comment) @comment
[
  "alias" "and" "begin" "break" "case" "class" "def" "defined?" "do"
  "else" "elsif" "end" "ensure" "for" "if" "in" "module" "next" "not"
  "or" "redo" "rescue" "retry" "return" "then" "undef" "unless" "until"
  "when" "while" "yield"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
"#;

pub const PHP_HIGHLIGHTS_SCM: &str = r#"
(function_definition name: (name) @function)
(method_declaration name: (name) @function.method)
(function_call_expression function: (name) @function)
(member_call_expression name: (name) @function.method)
(class_declaration name: (name) @type)
(interface_declaration name: (name) @type)
(trait_declaration name: (name) @type)
(variable_name) @variable
(comment) @comment
(string) @string
(encapsed_string) @string
(integer) @number
(float) @number
(boolean) @boolean
(null) @constant.builtin
[
  "abstract" "and" "array" "as" "break" "case" "catch" "class"
  "clone" "const" "continue" "declare" "default" "do" "echo" "else"
  "elseif" "enddeclare" "endfor" "endforeach" "endif" "endswitch"
  "endwhile" "enum" "extends" "final" "finally" "fn" "for" "foreach"
  "function" "global" "goto" "if" "implements" "include" "include_once"
  "instanceof" "insteadof" "interface" "match" "namespace" "new" "or"
  "print" "private" "protected" "public" "readonly" "require"
  "require_once" "return" "static" "switch" "throw" "trait" "try" "use"
  "while" "xor" "yield"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
"#;

pub const SWIFT_HIGHLIGHTS_SCM: &str = r#"
(type_identifier) @type
(function_declaration name: (simple_identifier) @function)
(class_declaration name: (type_identifier) @type)
(attribute) @attribute
(comment) @comment
(multiline_comment) @comment
(line_string_literal) @string
(multi_line_string_literal) @string
(integer_literal) @number
(real_literal) @number
(boolean_literal) @boolean
("nil") @constant.builtin
(else) @keyword
(throws) @keyword
[
  "actor" "as" "async" "await" "break" "case" "class" "continue"
  "deinit" "do" "enum" "extension" "fallthrough"
  "fileprivate" "for" "func" "guard" "if" "import" "in" "init" "inout"
  "internal" "is" "let" "open" "operator" "private" "protocol" "public"
  "repeat" "return" "self" "static" "struct" "subscript"
  "super" "switch" "try" "typealias" "var" "while"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
"#;

pub const ZIG_HIGHLIGHTS_SCM: &str = r#"
(function_declaration name: (identifier) @function)
(call_expression function: (identifier) @function)
(builtin_identifier) @attribute
(builtin_type) @type.builtin
(string) @string
(multiline_string) @string
(character) @string
(escape_sequence) @string.escape
(integer) @number
(float) @number
(boolean) @boolean
(comment) @comment
[
  "addrspace" "align" "allowzero" "and" "anyframe" "anytype" "asm"
  "async" "await" "break" "callconv" "catch" "comptime" "const"
  "continue" "defer" "else" "enum" "errdefer" "error" "export" "extern"
  "fn" "for" "if" "inline" "noalias" "noinline" "nosuspend" "opaque"
  "or" "orelse" "packed" "pub" "resume" "return" "struct" "suspend"
  "switch" "test" "threadlocal" "try" "union" "unreachable" "usingnamespace"
  "var" "volatile" "while"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
"#;

pub const LUA_HIGHLIGHTS_SCM: &str = r#"
(function_declaration name: (identifier) @function)
(function_call name: (identifier) @function)
(string) @string
(number) @number
(true) @boolean
(false) @boolean
(nil) @constant.builtin
(comment) @comment
(break_statement) @keyword
[
  "and" "do" "else" "elseif" "end" "for" "function" "goto" "if"
  "in" "local" "not" "or" "repeat" "return" "then" "until" "while"
] @keyword
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
"#;

pub const HTML_HIGHLIGHTS_SCM: &str = r#"
(tag_name) @tag
(attribute_name) @attribute
(attribute_value) @string
(quoted_attribute_value) @string
(comment) @comment
(doctype) @keyword
["<" ">" "</" "/>" "<!"] @punctuation.bracket
["="] @operator
"#;

pub const CSS_HIGHLIGHTS_SCM: &str = r##"
(tag_name) @tag
(class_name) @type
(id_name) @function
(property_name) @property
(string_value) @string
(color_value) @number
(integer_value) @number
(float_value) @number
(plain_value) @variable
(important) @keyword
(comment) @comment
["{" "}" "(" ")" "[" "]"] @punctuation.bracket
[":" ";" "," "." "#"] @punctuation.delimiter
"##;
