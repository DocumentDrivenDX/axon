use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Display},
    fs, io,
    path::{Path, PathBuf},
};

use clap::Args;
use proc_macro2::{Span, TokenStream, TokenTree};
use serde::{Deserialize, Serialize};
use sqlparser::{
    ast::Statement,
    dialect::{GenericDialect, MsSqlDialect, PostgreSqlDialect, SQLiteDialect},
    parser::Parser,
    tokenizer::Tokenizer,
};
use syn::{
    spanned::Spanned,
    visit::{self, Visit},
    Attribute, Expr, ExprCall, ExprMacro, ExprMethodCall, ExprPath, ImplItemFn, ItemFn, ItemMod,
    Lit, LitStr, Local, Macro, Pat, Path as SynPath,
};
use thiserror::Error;

#[derive(Debug, Clone, Args)]
pub struct AuditArgs {
    #[arg(
        long,
        value_name = "PATH",
        default_value = "crates/axon-storage/internal-manifest.json",
        help = "Audit manifest listing fixture roots, governed identifiers, and required records"
    )]
    pub manifest: PathBuf,
}

pub fn run(args: AuditArgs) -> Result<AuditReport, AuditError> {
    run_manifest(&args.manifest)
}

pub fn run_manifest(manifest_path: &Path) -> Result<AuditReport, AuditError> {
    let manifest = AuditManifest::read(manifest_path)?;
    let root = manifest.fixture_root(manifest_path);
    let governed = GovernedSet::from_manifest(&manifest);
    let mut observations = Vec::new();
    let mut findings = Vec::new();
    findings.extend(
        manifest
            .dynamic_sql_allowances
            .iter()
            .filter_map(DynamicSqlAllowance::validate),
    );
    findings.extend(
        manifest
            .excluded_functions
            .iter()
            .filter_map(ExcludedFunction::validate),
    );
    findings.extend(validate_routine_classifications(&manifest));

    for source in &manifest.rust_sources {
        let path = root.join(source);
        let content = read_to_string(&path)?;
        let parsed = syn::parse_file(&content).map_err(|source| AuditError::RustParse {
            path: path.clone(),
            source,
        })?;
        let mut visitor = RustSqlVisitor::new(
            source.clone(),
            path,
            &governed,
            &manifest.dynamic_sql_allowances,
            &manifest.excluded_functions,
        );
        visitor.visit_file(&parsed);
        observations.extend(visitor.observations);
        findings.extend(visitor.findings);
    }

    for source in &manifest.sql_sources {
        let path = root.join(&source.path);
        let sql = read_to_string(&path)?;
        let site = SourceSite {
            file: source.path.clone(),
            line: 1,
            enclosing_function: source.enclosing_function.clone(),
            entrypoint: "sql-file".to_owned(),
        };
        let source_kind = source.kind;
        inventory_sql(
            &site,
            &SqlText::Static(sql),
            source_kind,
            &governed,
            &manifest.dynamic_sql_allowances,
            &mut observations,
            &mut findings,
        );
    }

    Ok(check_manifest(manifest.records, observations, findings))
}

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("failed to read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("failed to parse JSON manifest {path}: {source}")]
    Manifest {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("failed to parse Rust source {path}: {source}")]
    RustParse { path: PathBuf, source: syn::Error },
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct CheckedRecord {
    pub source_file: String,
    pub enclosing_function: String,
    pub touched_tables: Vec<String>,
    pub touched_routines: Vec<String>,
    pub capability: String,
    pub mutation_class: String,
    pub co_commit: String,
    pub fault_test: String,
}

#[derive(Debug, Default)]
pub struct AuditReport {
    pub records: Vec<CheckedRecord>,
    pub findings: Vec<AuditFinding>,
}

impl AuditReport {
    pub fn is_success(&self) -> bool {
        self.findings.is_empty()
    }
}

impl Display for AuditReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.findings.is_empty() {
            writeln!(
                f,
                "audit-dml-boundary: checked {} governed SQL records",
                self.records.len()
            )
        } else {
            writeln!(
                f,
                "audit-dml-boundary found {} issue(s):",
                self.findings.len()
            )?;
            for finding in &self.findings {
                writeln!(f, "- {finding}")?;
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuditFinding {
    pub site: SourceSite,
    pub message: String,
}

impl Display for AuditFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.site.line == 0 {
            write!(
                f,
                "{} in {} via {}: {}",
                self.site.file, self.site.enclosing_function, self.site.entrypoint, self.message
            )
        } else {
            write!(
                f,
                "{}:{} in {} via {}: {}",
                self.site.file,
                self.site.line,
                self.site.enclosing_function,
                self.site.entrypoint,
                self.message
            )
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SourceSite {
    pub file: String,
    pub line: usize,
    pub enclosing_function: String,
    pub entrypoint: String,
}

#[derive(Debug, Default, Deserialize)]
struct AuditManifest {
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    rust_sources: Vec<String>,
    #[serde(default)]
    sql_sources: Vec<SqlSource>,
    #[serde(default)]
    governed_tables: Vec<String>,
    #[serde(default)]
    governed_routines: Vec<String>,
    #[serde(default)]
    routine_classifications: Vec<RoutineClassification>,
    #[serde(default)]
    records: Vec<CheckedRecord>,
    #[serde(default)]
    dynamic_sql_allowances: Vec<DynamicSqlAllowance>,
    #[serde(default)]
    excluded_functions: Vec<ExcludedFunction>,
}

impl AuditManifest {
    fn read(path: &Path) -> Result<Self, AuditError> {
        let raw = read_to_string(path)?;
        let file: AuditManifestFile =
            serde_json::from_str(&raw).map_err(|source| AuditError::Manifest {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(file.dml_boundary.unwrap_or(file.manifest))
    }

    fn fixture_root(&self, manifest_path: &Path) -> PathBuf {
        let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        match &self.root {
            Some(root) => manifest_dir.join(root),
            None => manifest_dir.to_path_buf(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AuditManifestFile {
    #[serde(default)]
    dml_boundary: Option<AuditManifest>,
    #[serde(flatten)]
    manifest: AuditManifest,
}

#[derive(Debug, Clone, Deserialize)]
struct RoutineClassification {
    name: String,
    class: RoutineClass,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RoutineClass {
    ReadOnly,
    Mutating,
    MigrationOnly,
}

impl Display for RoutineClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOnly => f.write_str("read_only"),
            Self::Mutating => f.write_str("mutating"),
            Self::MigrationOnly => f.write_str("migration_only"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct DynamicSqlAllowance {
    source_file: String,
    enclosing_function: String,
    entrypoint: String,
    allowance_class: String,
    reason: String,
}

impl DynamicSqlAllowance {
    fn matches(&self, site: &SourceSite) -> bool {
        normalize_path(&self.source_file) == normalize_path(&site.file)
            && self.enclosing_function == site.enclosing_function
            && self.entrypoint == site.entrypoint
    }

    fn validate(&self) -> Option<AuditFinding> {
        if self.allowance_class.trim().is_empty() || self.reason.trim().is_empty() {
            return Some(AuditFinding {
                site: SourceSite {
                    file: self.source_file.clone(),
                    line: 0,
                    enclosing_function: self.enclosing_function.clone(),
                    entrypoint: self.entrypoint.clone(),
                },
                message: "dynamic SQL allowance is missing class or reason".to_owned(),
            });
        }
        None
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ExcludedFunction {
    source_file: String,
    enclosing_function: String,
    reason: String,
}

impl ExcludedFunction {
    fn matches(&self, source_file: &str, enclosing_function: &str) -> bool {
        normalize_path(&self.source_file) == normalize_path(source_file)
            && self.enclosing_function == enclosing_function
    }

    fn validate(&self) -> Option<AuditFinding> {
        if self.reason.trim().is_empty() {
            return Some(AuditFinding {
                site: SourceSite {
                    file: self.source_file.clone(),
                    line: 0,
                    enclosing_function: self.enclosing_function.clone(),
                    entrypoint: "manifest".to_owned(),
                },
                message: "excluded function is missing reason".to_owned(),
            });
        }
        None
    }
}

#[derive(Debug, Deserialize)]
struct SqlSource {
    path: String,
    enclosing_function: String,
    kind: SourceKind,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SourceKind {
    Rust,
    Migration,
    DataOnly,
}

#[derive(Debug)]
struct GovernedSet {
    tables: HashSet<String>,
    routines: HashSet<String>,
}

impl GovernedSet {
    fn from_manifest(manifest: &AuditManifest) -> Self {
        Self {
            tables: manifest
                .governed_tables
                .iter()
                .map(|table| normalize_identifier(table))
                .collect(),
            routines: manifest
                .governed_routines
                .iter()
                .map(|routine| normalize_identifier(routine))
                .collect(),
        }
    }

    fn governs_table(&self, table: &str) -> bool {
        self.tables.contains(&normalize_identifier(table))
    }

    fn governs_routine(&self, routine: &str) -> bool {
        self.routines.contains(&normalize_identifier(routine))
    }

    fn mentioned_in(&self, sql: &str) -> bool {
        let lower = sql.to_ascii_lowercase();
        self.tables.iter().any(|table| lower.contains(table))
            || self.routines.iter().any(|routine| lower.contains(routine))
    }
}

fn validate_routine_classifications(manifest: &AuditManifest) -> Vec<AuditFinding> {
    let site = SourceSite {
        file: "manifest".to_owned(),
        line: 0,
        enclosing_function: "manifest".to_owned(),
        entrypoint: "routine_classifications".to_owned(),
    };
    let governed: HashSet<String> = manifest
        .governed_routines
        .iter()
        .map(|routine| normalize_identifier(routine))
        .collect();
    let mut seen = HashSet::new();
    let mut findings = Vec::new();

    for classification in &manifest.routine_classifications {
        let routine = normalize_identifier(&classification.name);
        if routine.is_empty() {
            findings.push(AuditFinding {
                site: site.clone(),
                message: format!(
                    "{} routine classification is missing a routine name",
                    classification.class
                ),
            });
            continue;
        }
        if !seen.insert(routine.clone()) {
            findings.push(AuditFinding {
                site: site.clone(),
                message: format!("duplicate routine classification for {routine}"),
            });
        }
        if !governed.contains(&routine) {
            findings.push(AuditFinding {
                site: site.clone(),
                message: format!(
                    "routine classification for {routine} does not match a governed routine"
                ),
            });
        }
    }

    for routine in governed {
        if !seen.contains(&routine) {
            findings.push(AuditFinding {
                site: site.clone(),
                message: format!(
                    "governed routine {routine} is missing routine classification \
                     (read_only, mutating, or migration_only)"
                ),
            });
        }
    }

    findings
}

#[derive(Debug, Clone)]
struct ObservedRecord {
    key: RecordKey,
    site: SourceSite,
    source_kind: SourceKind,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
struct RecordKey {
    source_file: String,
    enclosing_function: String,
    touched_tables: Vec<String>,
    touched_routines: Vec<String>,
    capability: String,
    mutation_class: String,
}

impl RecordKey {
    fn from_record(record: &CheckedRecord) -> Self {
        Self {
            source_file: normalize_path(&record.source_file),
            enclosing_function: record.enclosing_function.clone(),
            touched_tables: normalize_identifiers(&record.touched_tables),
            touched_routines: normalize_identifiers(&record.touched_routines),
            capability: record.capability.clone(),
            mutation_class: record.mutation_class.clone(),
        }
    }

    fn summary(&self) -> String {
        format!(
            "tables=[{}] routines=[{}] capability={} mutation_class={}",
            self.touched_tables.join(","),
            self.touched_routines.join(","),
            self.capability,
            self.mutation_class
        )
    }
}

fn check_manifest(
    expected_records: Vec<CheckedRecord>,
    observations: Vec<ObservedRecord>,
    mut findings: Vec<AuditFinding>,
) -> AuditReport {
    let mut expected = HashMap::new();

    for record in expected_records {
        let key = RecordKey::from_record(&record);
        let site = SourceSite {
            file: record.source_file.clone(),
            line: 0,
            enclosing_function: record.enclosing_function.clone(),
            entrypoint: "manifest".to_owned(),
        };

        if record.co_commit.trim().is_empty() {
            findings.push(AuditFinding {
                site: site.clone(),
                message: "manifest record is missing co-commit reference".to_owned(),
            });
        }
        if record.fault_test.trim().is_empty() {
            findings.push(AuditFinding {
                site: site.clone(),
                message: "manifest record is missing fault-test reference".to_owned(),
            });
        }
        if expected.insert(key, record).is_some() {
            findings.push(AuditFinding {
                site,
                message: "duplicate manifest record for governed SQL site".to_owned(),
            });
        }
    }

    let mut checked = Vec::new();
    let mut seen_observations = HashSet::new();
    for observation in observations {
        if !seen_observations.insert(observation.key.clone()) {
            continue;
        }
        if let Some(record) = expected.remove(&observation.key) {
            checked.push(record);
        } else {
            findings.push(AuditFinding {
                site: observation.site,
                message: format!(
                    "{}: {}",
                    unmanifested_message(observation.source_kind),
                    observation.key.summary()
                ),
            });
        }
    }

    for (_, record) in expected {
        findings.push(AuditFinding {
            site: SourceSite {
                file: record.source_file,
                line: 0,
                enclosing_function: record.enclosing_function,
                entrypoint: "manifest".to_owned(),
            },
            message: "manifested governed SQL record was not found in fixtures".to_owned(),
        });
    }

    checked.sort_by(|left, right| {
        (
            &left.source_file,
            &left.enclosing_function,
            &left.mutation_class,
            &left.capability,
        )
            .cmp(&(
                &right.source_file,
                &right.enclosing_function,
                &right.mutation_class,
                &right.capability,
            ))
    });
    findings.sort_by(|left, right| {
        (
            &left.site.file,
            left.site.line,
            &left.site.enclosing_function,
            &left.message,
        )
            .cmp(&(
                &right.site.file,
                right.site.line,
                &right.site.enclosing_function,
                &right.message,
            ))
    });

    AuditReport {
        records: checked,
        findings,
    }
}

fn unmanifested_message(kind: SourceKind) -> String {
    match kind {
        SourceKind::DataOnly => "unmanifested data-only DML touches governed state".to_owned(),
        SourceKind::Migration => "unmanifested migration SQL touches governed state".to_owned(),
        SourceKind::Rust => "unmanifested governed DML entrypoint".to_owned(),
    }
}

#[derive(Debug)]
struct RustSqlVisitor<'a> {
    source_file: String,
    file_path: PathBuf,
    function_stack: Vec<String>,
    local_sql_stack: Vec<HashMap<String, SqlText>>,
    governed: &'a GovernedSet,
    dynamic_sql_allowances: &'a [DynamicSqlAllowance],
    excluded_functions: &'a [ExcludedFunction],
    observations: Vec<ObservedRecord>,
    findings: Vec<AuditFinding>,
}

impl<'a> RustSqlVisitor<'a> {
    fn new(
        source_file: String,
        file_path: PathBuf,
        governed: &'a GovernedSet,
        dynamic_sql_allowances: &'a [DynamicSqlAllowance],
        excluded_functions: &'a [ExcludedFunction],
    ) -> Self {
        Self {
            source_file,
            file_path,
            function_stack: Vec::new(),
            local_sql_stack: Vec::new(),
            governed,
            dynamic_sql_allowances,
            excluded_functions,
            observations: Vec::new(),
            findings: Vec::new(),
        }
    }

    fn current_function(&self) -> String {
        self.function_stack
            .last()
            .cloned()
            .unwrap_or_else(|| "<module>".to_owned())
    }

    fn site(&self, span: Span, entrypoint: &str) -> SourceSite {
        SourceSite {
            file: self.source_file.clone(),
            line: span.start().line,
            enclosing_function: self.current_function(),
            entrypoint: entrypoint.to_owned(),
        }
    }

    fn inventory_expr(&mut self, site: SourceSite, expr: &Expr) {
        let sql = extract_sql_expr(expr, &self.file_path, self.current_local_sql());
        inventory_sql(
            &site,
            &sql,
            SourceKind::Rust,
            self.governed,
            self.dynamic_sql_allowances,
            &mut self.observations,
            &mut self.findings,
        );
    }

    fn inventory_macro(&mut self, mac: &Macro) {
        let entrypoint = path_signature(&mac.path);
        if !is_sqlx_macro(&mac.path) {
            return;
        }
        let site = self.site(mac.path.span(), &entrypoint);
        let sql = first_lit_str(&mac.tokens)
            .map(|lit| SqlText::Static(lit.value()))
            .unwrap_or_else(|| {
                SqlText::Dynamic("macro SQL argument is not a string literal".into())
            });
        inventory_sql(
            &site,
            &sql,
            SourceKind::Rust,
            self.governed,
            self.dynamic_sql_allowances,
            &mut self.observations,
            &mut self.findings,
        );
    }

    fn current_local_sql(&self) -> Option<&HashMap<String, SqlText>> {
        self.local_sql_stack.last()
    }

    fn current_local_sql_mut(&mut self) -> Option<&mut HashMap<String, SqlText>> {
        self.local_sql_stack.last_mut()
    }

    fn function_excluded(&self, function: &str) -> bool {
        self.excluded_functions
            .iter()
            .any(|excluded| excluded.matches(&self.source_file, function))
    }
}

impl<'ast> Visit<'ast> for RustSqlVisitor<'_> {
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if has_cfg_test(&item.attrs) {
            return;
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if has_cfg_test(&item.attrs) {
            return;
        }
        let function = item.sig.ident.to_string();
        if self.function_excluded(&function) {
            return;
        }
        self.function_stack.push(function);
        self.local_sql_stack.push(HashMap::new());
        visit::visit_item_fn(self, item);
        self.local_sql_stack.pop();
        self.function_stack.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        if has_cfg_test(&item.attrs) {
            return;
        }
        let function = item.sig.ident.to_string();
        if self.function_excluded(&function) {
            return;
        }
        self.function_stack.push(function);
        self.local_sql_stack.push(HashMap::new());
        visit::visit_impl_item_fn(self, item);
        self.local_sql_stack.pop();
        self.function_stack.pop();
    }

    fn visit_local(&mut self, node: &'ast Local) {
        if let Pat::Ident(ident) = &node.pat {
            if let Some(init) = &node.init {
                let sql = extract_sql_initializer(&init.expr, &self.file_path);
                if let Some(sql) = sql {
                    if let Some(locals) = self.current_local_sql_mut() {
                        locals.insert(ident.ident.to_string(), sql);
                    }
                }
            }
        }
        visit::visit_local(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = node.func.as_ref() {
            if let Some(entrypoint) = call_entrypoint(path) {
                if let Some(sql_arg) = node.args.first() {
                    let site = self.site(node.span(), &entrypoint);
                    self.inventory_expr(site, sql_arg);
                }
            }
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method = node.method.to_string();
        if is_sql_method_entrypoint(&method) {
            if let Some(sql_arg) = node.args.first() {
                if method_accepts_sql_argument(&method, sql_arg) {
                    let site = self.site(node.span(), &method);
                    self.inventory_expr(site, sql_arg);
                }
            }
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        self.inventory_macro(&node.mac);
        visit::visit_expr_macro(self, node);
    }
}

#[derive(Debug, Clone)]
enum SqlText {
    Static(String),
    Dynamic(String),
}

fn extract_sql_expr(
    expr: &Expr,
    source_path: &Path,
    local_sql: Option<&HashMap<String, SqlText>>,
) -> SqlText {
    match expr {
        Expr::Lit(expr_lit) => match &expr_lit.lit {
            Lit::Str(value) => SqlText::Static(value.value()),
            _ => SqlText::Dynamic("SQL argument is not a string literal".to_owned()),
        },
        Expr::Reference(reference) => extract_sql_expr(&reference.expr, source_path, local_sql),
        Expr::Paren(paren) => extract_sql_expr(&paren.expr, source_path, local_sql),
        Expr::Group(group) => extract_sql_expr(&group.expr, source_path, local_sql),
        Expr::Path(path) => local_sql
            .and_then(|locals| single_ident(path).and_then(|ident| locals.get(&ident).cloned()))
            .unwrap_or_else(|| {
                SqlText::Dynamic("SQL argument is not statically auditable".to_owned())
            }),
        Expr::Macro(expr_macro) if path_ends_with(&expr_macro.mac.path, &["include_str"]) => {
            extract_include_str(&expr_macro.mac, source_path)
        }
        Expr::Macro(expr_macro) if path_ends_with(&expr_macro.mac.path, &["format"]) => {
            SqlText::Dynamic(
                first_lit_str(&expr_macro.mac.tokens)
                    .map(|lit| lit.value())
                    .unwrap_or_else(|| "format! SQL argument is not a string literal".to_owned()),
            )
        }
        _ => SqlText::Dynamic("SQL argument is not statically auditable".to_owned()),
    }
}

fn extract_sql_initializer(expr: &Expr, source_path: &Path) -> Option<SqlText> {
    match expr {
        Expr::Lit(expr_lit) => match &expr_lit.lit {
            Lit::Str(value) => Some(SqlText::Static(value.value())),
            _ => None,
        },
        Expr::Call(call) => {
            if let Expr::Path(path) = call.func.as_ref() {
                if path_ends_with(&path.path, &["String", "from"]) {
                    return call
                        .args
                        .first()
                        .map(|arg| extract_sql_expr(arg, source_path, None));
                }
            }
            None
        }
        Expr::Macro(expr_macro)
            if path_ends_with(&expr_macro.mac.path, &["format"])
                || path_ends_with(&expr_macro.mac.path, &["include_str"]) =>
        {
            Some(extract_sql_expr(expr, source_path, None))
        }
        _ => None,
    }
}

fn single_ident(path: &ExprPath) -> Option<String> {
    if path.qself.is_none() && path.path.segments.len() == 1 {
        path.path
            .segments
            .first()
            .map(|segment| segment.ident.to_string())
    } else {
        None
    }
}

fn extract_include_str(mac: &Macro, source_path: &Path) -> SqlText {
    let Some(lit) = first_lit_str(&mac.tokens) else {
        return SqlText::Dynamic("include_str! path is not a string literal".to_owned());
    };
    let include_path = source_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(lit.value());
    match fs::read_to_string(&include_path) {
        Ok(sql) => SqlText::Static(sql),
        Err(error) => SqlText::Dynamic(format!(
            "include_str! target {} could not be read: {error}",
            include_path.display()
        )),
    }
}

fn call_entrypoint(path: &ExprPath) -> Option<String> {
    let syn_path = &path.path;
    if path_ends_with(syn_path, &["sqlx", "query"])
        || path_ends_with(syn_path, &["sqlx", "query_scalar"])
        || path_ends_with(syn_path, &["sqlx", "query_as"])
        || path_ends_with(syn_path, &["sqlx", "raw_sql"])
        || is_query_builder_new(syn_path)
    {
        Some(path_signature(syn_path))
    } else {
        None
    }
}

fn is_query_builder_new(path: &SynPath) -> bool {
    let segments: Vec<_> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    segments.last().is_some_and(|last| last == "new")
        && segments.iter().any(|segment| segment == "QueryBuilder")
}

fn is_sql_method_entrypoint(method: &str) -> bool {
    matches!(
        method,
        "execute" | "execute_batch" | "batch_execute" | "copy_in_raw" | "copy_out_raw"
    )
}

fn method_accepts_sql_argument(method: &str, expr: &Expr) -> bool {
    if matches!(method, "copy_in_raw" | "copy_out_raw") {
        return true;
    }
    matches!(method, "execute" | "execute_batch" | "batch_execute") && is_sql_literal_like(expr)
}

fn is_sql_literal_like(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(expr_lit) => matches!(expr_lit.lit, Lit::Str(_)),
        Expr::Reference(reference) => is_sql_literal_like(&reference.expr),
        Expr::Paren(paren) => is_sql_literal_like(&paren.expr),
        Expr::Group(group) => is_sql_literal_like(&group.expr),
        Expr::Macro(expr_macro) => {
            path_ends_with(&expr_macro.mac.path, &["format"])
                || path_ends_with(&expr_macro.mac.path, &["include_str"])
        }
        _ => false,
    }
}

fn has_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .meta
                .require_list()
                .is_ok_and(|list| list.tokens.to_string().contains("test"))
    })
}

fn is_sqlx_macro(path: &SynPath) -> bool {
    path_ends_with(path, &["sqlx", "query"])
        || path_ends_with(path, &["sqlx", "query_scalar"])
        || path_ends_with(path, &["sqlx", "query_as"])
        || path_ends_with(path, &["sqlx", "raw_sql"])
}

fn path_ends_with(path: &SynPath, suffix: &[&str]) -> bool {
    let segments: Vec<_> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    if segments.len() < suffix.len() {
        return false;
    }
    segments[segments.len() - suffix.len()..]
        .iter()
        .zip(suffix)
        .all(|(segment, expected)| segment == expected)
}

fn path_signature(path: &SynPath) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn first_lit_str(tokens: &TokenStream) -> Option<LitStr> {
    tokens.clone().into_iter().find_map(|token| match token {
        TokenTree::Literal(literal) => syn::parse_str::<LitStr>(&literal.to_string()).ok(),
        _ => None,
    })
}

fn inventory_sql(
    site: &SourceSite,
    sql: &SqlText,
    source_kind: SourceKind,
    governed: &GovernedSet,
    dynamic_sql_allowances: &[DynamicSqlAllowance],
    observations: &mut Vec<ObservedRecord>,
    findings: &mut Vec<AuditFinding>,
) {
    let sql = match sql {
        SqlText::Static(sql) => sql,
        SqlText::Dynamic(reason) => {
            if !looks_like_mutation(reason)
                && dynamic_sql_allowances
                    .iter()
                    .any(|allowance| allowance.matches(site))
            {
                return;
            }
            findings.push(AuditFinding {
                site: site.clone(),
                message: dynamic_sql_message(reason, governed),
            });
            return;
        }
    };

    if !looks_like_auditable_sql(sql) {
        return;
    }

    let statements = match parse_sql(sql) {
        Ok(statements) => statements,
        Err(message) => {
            if governed.mentioned_in(sql) || looks_like_mutation(sql) {
                findings.push(AuditFinding {
                    site: site.clone(),
                    message: format!("unparseable governed DML: {message}"),
                });
            }
            return;
        }
    };

    for statement in statements {
        if let Some(key) = classify_statement(&statement, site, governed) {
            observations.push(ObservedRecord {
                key,
                site: site.clone(),
                source_kind,
            });
        }
    }
}

fn dynamic_sql_message(reason: &str, governed: &GovernedSet) -> String {
    if looks_like_mutation(reason) || governed.mentioned_in(reason) {
        format!("dynamic governed DML identifier is not auditable: {reason}")
    } else {
        format!("dynamic SQL requires manifest allowance: {reason}")
    }
}

fn parse_sql(sql: &str) -> Result<Vec<Statement>, String> {
    let postgres = PostgreSqlDialect {};
    let sqlite = SQLiteDialect {};
    let generic = GenericDialect {};
    let mssql = MsSqlDialect {};

    Parser::parse_sql(&postgres, sql)
        .or_else(|postgres_error| {
            Parser::parse_sql(&sqlite, sql).map_err(|sqlite_error| {
                format!("PostgreSQL parser: {postgres_error}; SQLite parser: {sqlite_error}")
            })
        })
        .or_else(|prior_error| {
            Parser::parse_sql(&generic, sql)
                .map_err(|generic_error| format!("{prior_error}; generic parser: {generic_error}"))
        })
        .or_else(|prior_error| {
            Parser::parse_sql(&mssql, sql)
                .map_err(|mssql_error| format!("{prior_error}; MSSQL parser: {mssql_error}"))
        })
}

fn classify_statement(
    statement: &Statement,
    site: &SourceSite,
    governed: &GovernedSet,
) -> Option<RecordKey> {
    let sql = statement.to_string();
    let tokens = statement_tokens(&sql).ok()?;
    let first = token_keyword(tokens.first()?)?;

    match first.as_str() {
        "INSERT" => table_record(site, "insert", table_after(&tokens, "INTO")?, governed),
        "UPDATE" => table_record(site, "update", object_after_index(&tokens, 1)?, governed),
        "DELETE" => table_record(site, "delete", table_after(&tokens, "FROM")?, governed),
        "COPY" => table_record(site, "copy", object_after_index(&tokens, 1)?, governed),
        "CREATE" => classify_create(site, &tokens, governed),
        _ => None,
    }
}

fn classify_create(
    site: &SourceSite,
    tokens: &[String],
    governed: &GovernedSet,
) -> Option<RecordKey> {
    let keyword_index = create_subject_index(tokens)?;
    let subject = token_keyword(tokens.get(keyword_index)?)?;
    match subject.as_str() {
        "TABLE" => table_record(
            site,
            "create_table",
            create_object_after_subject(tokens, keyword_index)?,
            governed,
        ),
        "TRIGGER" => {
            let routine = create_object_after_subject(tokens, keyword_index)?;
            let table = table_after(tokens, "ON")?;
            if !governed.governs_routine(&routine) && !governed.governs_table(&table) {
                return None;
            }
            Some(RecordKey {
                source_file: normalize_path(&site.file),
                enclosing_function: site.enclosing_function.clone(),
                touched_tables: normalize_identifiers(&[table]),
                touched_routines: normalize_identifiers(std::slice::from_ref(&routine)),
                capability: format!("routine:{}:manage", normalize_identifier(&routine)),
                mutation_class: "create_trigger".to_owned(),
            })
        }
        "FUNCTION" => routine_record(
            site,
            "create_function",
            create_object_after_subject(tokens, keyword_index)?,
            governed,
        ),
        "PROCEDURE" | "PROC" => routine_record(
            site,
            "create_procedure",
            create_object_after_subject(tokens, keyword_index)?,
            governed,
        ),
        _ => None,
    }
}

fn create_object_after_subject(tokens: &[String], subject_index: usize) -> Option<String> {
    let mut object_index = subject_index + 1;
    if token_matches(tokens.get(object_index)?, "IF") {
        object_index += 1;
        if token_matches(tokens.get(object_index)?, "NOT") {
            object_index += 1;
        }
        if token_matches(tokens.get(object_index)?, "EXISTS") {
            object_index += 1;
        }
    }
    object_after_index(tokens, object_index)
}

fn table_record(
    site: &SourceSite,
    mutation_class: &str,
    table: String,
    governed: &GovernedSet,
) -> Option<RecordKey> {
    if !governed.governs_table(&table) {
        return None;
    }
    let table = normalize_identifier(&table);
    Some(RecordKey {
        source_file: normalize_path(&site.file),
        enclosing_function: site.enclosing_function.clone(),
        touched_tables: vec![table.clone()],
        touched_routines: Vec::new(),
        capability: format!("table:{table}:{mutation_class}"),
        mutation_class: mutation_class.to_owned(),
    })
}

fn routine_record(
    site: &SourceSite,
    mutation_class: &str,
    routine: String,
    governed: &GovernedSet,
) -> Option<RecordKey> {
    if !governed.governs_routine(&routine) {
        return None;
    }
    let routine = normalize_identifier(&routine);
    Some(RecordKey {
        source_file: normalize_path(&site.file),
        enclosing_function: site.enclosing_function.clone(),
        touched_tables: Vec::new(),
        touched_routines: vec![routine.clone()],
        capability: format!("routine:{routine}:manage"),
        mutation_class: mutation_class.to_owned(),
    })
}

fn statement_tokens(sql: &str) -> Result<Vec<String>, String> {
    let dialect = PostgreSqlDialect {};
    Tokenizer::new(&dialect, sql)
        .tokenize()
        .map_err(|error| error.to_string())
        .map(|tokens| {
            tokens
                .into_iter()
                .map(|token| token.to_string())
                .filter(|token| !token.trim().is_empty() && token != ";")
                .collect()
        })
}

fn create_subject_index(tokens: &[String]) -> Option<usize> {
    let mut index = 1;
    if token_matches(tokens.get(index)?, "OR") {
        index += 1;
        if token_matches(tokens.get(index)?, "REPLACE")
            || token_matches(tokens.get(index)?, "ALTER")
        {
            index += 1;
        }
    }
    Some(index)
}

fn table_after(tokens: &[String], keyword: &str) -> Option<String> {
    tokens
        .iter()
        .position(|token| token_matches_str(token, keyword))
        .and_then(|index| object_after_index(tokens, index + 1))
}

fn object_after_index(tokens: &[String], start: usize) -> Option<String> {
    let mut index = start;
    if token_matches(tokens.get(index)?, "ONLY") {
        index += 1;
    }

    let mut name = String::new();
    while let Some(token) = tokens.get(index) {
        if token == "." {
            name.push('.');
            index += 1;
            continue;
        }
        if is_object_name_stop(token) {
            break;
        }
        if is_identifier_token(token) {
            if !name.is_empty() && !name.ends_with('.') {
                break;
            }
            name.push_str(&strip_identifier_quotes(token));
            index += 1;
            continue;
        }
        break;
    }

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn is_identifier_token(token: &str) -> bool {
    let stripped = strip_identifier_quotes(token);
    !stripped.is_empty()
        && stripped
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$'))
        && !is_object_name_stop(&stripped)
}

fn is_object_name_stop(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "(" | ")"
            | ","
            | "VALUES"
            | "VALUE"
            | "SET"
            | "FROM"
            | "TO"
            | "ON"
            | "USING"
            | "AS"
            | "RETURNS"
            | "RETURN"
            | "LANGUAGE"
            | "BEGIN"
            | "END"
            | "BEFORE"
            | "AFTER"
            | "INSTEAD"
            | "FOR"
            | "WHEN"
            | "EXECUTE"
            | "WITH"
            | "WHERE"
    )
}

fn token_keyword(token: &str) -> Option<String> {
    if is_identifier_token(token) {
        Some(strip_identifier_quotes(token).to_ascii_uppercase())
    } else {
        None
    }
}

fn token_matches(token: &str, expected: &str) -> bool {
    token_matches_str(token, expected)
}

fn token_matches_str(token: &str, expected: &str) -> bool {
    strip_identifier_quotes(token).eq_ignore_ascii_case(expected)
}

fn looks_like_auditable_sql(sql: &str) -> bool {
    looks_like_mutation(sql) || sql.to_ascii_lowercase().contains("create trigger")
}

fn looks_like_mutation(sql: &str) -> bool {
    let trimmed = sql.trim_start().to_ascii_uppercase();
    [
        "INSERT", "UPDATE", "DELETE", "COPY", "CREATE", "MERGE", "ALTER", "DROP",
    ]
    .iter()
    .any(|keyword| trimmed.starts_with(keyword))
}

fn normalize_identifiers<S: AsRef<str>>(values: &[S]) -> Vec<String> {
    let mut normalized: Vec<_> = values
        .iter()
        .map(|value| normalize_identifier(value.as_ref()))
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_identifier(value: &str) -> String {
    strip_identifier_quotes(value)
        .rsplit('.')
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn strip_identifier_quotes(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('`')
        .trim_matches('[')
        .trim_matches(']')
        .to_owned()
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn read_to_string(path: &Path) -> Result<String, AuditError> {
    fs::read_to_string(path).map_err(|source| AuditError::Read {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::{run_manifest, CheckedRecord};

    fn fixture_manifest(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audit_dml_boundary")
            .join(name)
            .join("manifest.json")
    }

    fn repository_manifest() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask should live under repository root")
            .join("crates/axon-storage/internal-manifest.json")
    }

    #[test]
    fn audit_dml_boundary_fixtures() {
        let report = run_manifest(&fixture_manifest("valid")).expect("valid fixtures audit");
        assert!(
            report.is_success(),
            "expected valid fixtures to pass:\n{report}"
        );
        assert_eq!(13, report.records.len());

        assert_record(
            &report.records,
            "src/entrypoints.rs",
            "copy_entities",
            "copy",
            "table:entity_imports:copy",
        );
        assert_record(
            &report.records,
            "src/entrypoints.rs",
            "query_builder_insert",
            "insert",
            "table:audit_events:insert",
        );
        assert_record(
            &report.records,
            "migrations/001_schema.sql",
            "<migration>",
            "create_trigger",
            "routine:enforce_entity_write:manage",
        );
        assert_record(
            &report.records,
            "migrations/001_schema.sql",
            "<migration>",
            "create_function",
            "routine:audit_entity_mutation:manage",
        );
        assert_record(
            &report.records,
            "migrations/001_schema.sql",
            "<migration>",
            "create_procedure",
            "routine:refresh_entity_projection:manage",
        );
        assert_record(
            &report.records,
            "data/seed.sql",
            "<data-only>",
            "insert",
            "table:data_seed:insert",
        );
    }

    #[test]
    fn audit_dml_boundary_invalid_fixtures() {
        let report = run_manifest(&fixture_manifest("invalid")).expect("invalid fixtures audit");
        assert!(
            !report.is_success(),
            "expected invalid fixtures to fail the audit"
        );

        let rendered = report.to_string();
        assert!(
            rendered.contains("src/dynamic.rs") && rendered.contains("dynamic_table"),
            "dynamic governed DML error was not source-site-specific:\n{rendered}"
        );
        assert!(
            rendered.contains("dynamic governed DML identifier"),
            "missing dynamic governed DML finding:\n{rendered}"
        );
        assert!(
            rendered.contains("src/unparseable.rs") && rendered.contains("broken_sql"),
            "unparseable governed DML error was not source-site-specific:\n{rendered}"
        );
        assert!(
            rendered.contains("unparseable governed DML"),
            "missing unparseable governed DML finding:\n{rendered}"
        );
        assert!(
            rendered.contains("data/seed.sql")
                && rendered.contains("<data-only>")
                && rendered.contains("unmanifested data-only DML"),
            "missing unmanifested data-only DML finding:\n{rendered}"
        );
    }

    #[test]
    fn audit_dml_boundary_rejects_unclassified_routines() {
        let report = run_manifest(&fixture_manifest("unclassified_routine"))
            .expect("unclassified routine fixture");
        assert!(
            !report.is_success(),
            "expected unclassified routine fixture to fail the audit"
        );

        let rendered = report.to_string();
        assert!(
            rendered.contains("unsafe_mutator")
                && rendered.contains("missing routine classification"),
            "missing unclassified routine finding:\n{rendered}"
        );
    }

    #[test]
    fn audit_dml_boundary_repository_inventory() {
        let report = run_manifest(&repository_manifest()).expect("repository audit");
        assert!(
            report.is_success(),
            "expected repository DML boundary manifest to pass:\n{report}"
        );
        assert!(
            report.records.len() >= 100,
            "repository audit should cover live storage SQL, got {} records",
            report.records.len()
        );
        assert_record(
            &report.records,
            "crates/axon-storage/src/auth_schema.rs",
            "apply_auth_migrations_sqlite",
            "create_table",
            "table:tenants:create_table",
        );
        assert_record(
            &report.records,
            "crates/axon-storage/src/postgres.rs",
            "ensure_postgres_routines",
            "create_function",
            "routine:axon_record_mutation_intent:manage",
        );
        assert_record(
            &report.records,
            "crates/axon-storage/src/postgres.rs",
            "put",
            "insert",
            "table:entities:insert",
        );
        assert_record(
            &report.records,
            "crates/axon-storage/src/sqlite.rs",
            "drop_namespace",
            "delete",
            "table:namespaces:delete",
        );
    }

    #[test]
    fn audit_dml_boundary_repository_negative_fixture() {
        let report = run_manifest(&fixture_manifest("repository_negative"))
            .expect("negative repository fixture");
        assert!(
            !report.is_success(),
            "expected repository negative fixture to fail"
        );

        let rendered = report.to_string();
        assert!(
            rendered.contains("data/direct_entity_write.sql:1")
                && rendered.contains("<data-only>")
                && rendered.contains("unmanifested data-only DML")
                && rendered.contains("table:entities:insert"),
            "negative fixture should name the direct governed data-only write site:\n{rendered}"
        );
    }

    fn assert_record(
        records: &[CheckedRecord],
        source_file: &str,
        enclosing_function: &str,
        mutation_class: &str,
        capability: &str,
    ) {
        assert!(
            records.iter().any(|record| {
                record.source_file == source_file
                    && record.enclosing_function == enclosing_function
                    && record.mutation_class == mutation_class
                    && record.capability == capability
                    && !record.co_commit.is_empty()
                    && !record.fault_test.is_empty()
            }),
            "record not found: {source_file} {enclosing_function} {mutation_class} {capability}"
        );
    }
}
