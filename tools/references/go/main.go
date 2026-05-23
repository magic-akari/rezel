package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"go/ast"
	"go/parser"
	"go/scanner"
	"go/token"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strings"
	"unicode/utf8"
)

const (
	requiredGoVersion = "go1.26.3"
	snapshotSchema    = "rezel.go-parser-reference-snapshot.v1"
	stdlibSchema      = "rezel.go-stdlib-ast-fingerprints.v1"
	fnvPrime          = uint64(0x0000_0100_0000_01b3)
)

var (
	astNodeType  = reflect.TypeFor[ast.Node]()
	chanDirType  = reflect.TypeFor[ast.ChanDir]()
	tokenPosType = reflect.TypeFor[token.Pos]()
	tokenType    = reflect.TypeFor[token.Token]()
)

var parseMode = parser.AllErrors | parser.ParseComments | parser.SkipObjectResolution

var parseModeNames = []string{
	"AllErrors",
	"ParseComments",
	"SkipObjectResolution",
}

var excludedFields = []string{
	"File.Scope",
	"File.Unresolved",
	"Ident.Obj",
}

var recoveryOnlyNodeKinds = []string{
	"BadDecl",
	"BadExpr",
	"BadStmt",
}

var outOfScopeNodeKinds = []string{
	"Directive",
	"Package",
}

var requiredNodeKinds = []string{
	"ArrayType",
	"AssignStmt",
	"BasicLit",
	"BinaryExpr",
	"BlockStmt",
	"BranchStmt",
	"CallExpr",
	"CaseClause",
	"ChanType",
	"CommClause",
	"Comment",
	"CommentGroup",
	"CompositeLit",
	"DeclStmt",
	"DeferStmt",
	"Ellipsis",
	"EmptyStmt",
	"ExprStmt",
	"Field",
	"FieldList",
	"File",
	"ForStmt",
	"FuncDecl",
	"FuncLit",
	"FuncType",
	"GenDecl",
	"GoStmt",
	"Ident",
	"IfStmt",
	"ImportSpec",
	"IncDecStmt",
	"IndexExpr",
	"IndexListExpr",
	"InterfaceType",
	"KeyValueExpr",
	"LabeledStmt",
	"MapType",
	"ParenExpr",
	"RangeStmt",
	"ReturnStmt",
	"SelectStmt",
	"SelectorExpr",
	"SendStmt",
	"SliceExpr",
	"StarExpr",
	"StructType",
	"SwitchStmt",
	"TypeAssertExpr",
	"TypeSpec",
	"TypeSwitchStmt",
	"UnaryExpr",
	"ValueSpec",
}

type snapshot struct {
	Schema                string         `json:"schema"`
	GoVersion             string         `json:"goVersion"`
	Coordinates           string         `json:"coordinates"`
	ParseMode             []string       `json:"parseMode"`
	ExcludedFields        []string       `json:"excludedFields"`
	RecoveryOnlyNodeKinds []string       `json:"recoveryOnlyNodeKinds"`
	OutOfScopeNodeKinds   []string       `json:"outOfScopeNodeKinds"`
	NodeKinds             []string       `json:"nodeKinds"`
	Accepted              []acceptedCase `json:"accepted"`
	Rejected              []rejectedCase `json:"rejected"`
}

type fixture struct {
	ID         string
	SourceName string
	Source     string
}

type acceptedCase struct {
	ID         string         `json:"id"`
	SourceName string         `json:"sourceName"`
	Source     string         `json:"source"`
	AST        normalizedNode `json:"ast"`
}

type rejectedCase struct {
	ID         string `json:"id"`
	SourceName string `json:"sourceName"`
	Source     string `json:"source"`
}

type normalizedNode struct {
	Kind   string            `json:"kind"`
	Start  *int              `json:"start"`
	End    *int              `json:"end"`
	Fields []normalizedField `json:"fields"`
}

type normalizedField struct {
	Field string `json:"field"`
	Value any    `json:"value"`
}

type stdlibOracle struct {
	Schema         string              `json:"schema"`
	GoVersion      string              `json:"goVersion"`
	Coordinates    string              `json:"coordinates"`
	ParseMode      []string            `json:"parseMode"`
	ExcludedFields []string            `json:"excludedFields"`
	Sources        []stdlibFingerprint `json:"sources"`
}

type stdlibFingerprint struct {
	Path        string  `json:"path"`
	Accepted    bool    `json:"accepted"`
	ErrorCount  int     `json:"errorCount"`
	Nodes       int     `json:"nodes"`
	Fingerprint *string `json:"fingerprint"`
}

type normalizer struct {
	files     *token.FileSet
	nodeKinds map[string]struct{}
}

type astFingerprint struct {
	state uint64
	nodes int
}

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(arguments []string) error {
	if runtime.Version() != requiredGoVersion {
		return fmt.Errorf("%s is required, found %s", requiredGoVersion, runtime.Version())
	}
	if len(arguments) == 3 && arguments[0] == "--stdlib-fingerprints" {
		return writeStdlibFingerprints(arguments[1], arguments[2])
	}
	update, err := parseArguments(arguments)
	if err != nil {
		return err
	}

	current, err := buildSnapshot(".")
	if err != nil {
		return err
	}
	encoded, err := renderSnapshot(current)
	if err != nil {
		return err
	}

	path := filepath.Join("snapshots", "go.json")
	if update {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return fmt.Errorf("create snapshot directory: %w", err)
		}
		if err := os.WriteFile(path, encoded, 0o644); err != nil {
			return fmt.Errorf("write snapshot: %w", err)
		}
		fmt.Fprintf(os.Stderr, "updated %s\n", path)
		return nil
	}

	checked, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("read snapshot: %w", err)
	}
	if !bytes.Equal(checked, encoded) {
		return errors.New("Go parser reference snapshot is stale; run with --update")
	}
	return nil
}

func writeStdlibFingerprints(root string, output string) error {
	root, err := filepath.Abs(root)
	if err != nil {
		return fmt.Errorf("resolve Go source root: %w", err)
	}
	sources, err := goSources(root)
	if err != nil {
		return err
	}
	records := make([]stdlibFingerprint, 0, len(sources))
	for index, path := range sources {
		source, err := os.ReadFile(path)
		if err != nil {
			return fmt.Errorf("read %s: %w", path, err)
		}
		files := token.NewFileSet()
		file, parseErr := parser.ParseFile(files, path, source, parseMode)
		relative, err := filepath.Rel(root, path)
		if err != nil {
			return fmt.Errorf("resolve relative Go source path: %w", err)
		}
		record := stdlibFingerprint{
			Path:       filepath.ToSlash(relative),
			Accepted:   parseErr == nil && file != nil,
			ErrorCount: parseErrorCount(parseErr),
		}
		if record.Accepted {
			projector := normalizer{
				files:     files,
				nodeKinds: make(map[string]struct{}),
			}
			projected, err := projector.node(file)
			if err != nil {
				return fmt.Errorf("%s: %w", record.Path, err)
			}
			if projected == nil {
				return fmt.Errorf("%s produced a nil AST projection", record.Path)
			}
			fingerprint := astFingerprint{}
			if err := fingerprint.addNode(projected); err != nil {
				return fmt.Errorf("%s: %w", record.Path, err)
			}
			hexadecimal := fingerprint.hexadecimal()
			record.Nodes = fingerprint.nodes
			record.Fingerprint = &hexadecimal
		}
		records = append(records, record)
		if (index+1)%1_000 == 0 {
			fmt.Fprintf(os.Stderr, "go/parser AST fingerprints: %d/%d\n", index+1, len(sources))
		}
	}

	if err := os.MkdirAll(filepath.Dir(output), 0o755); err != nil {
		return fmt.Errorf("create Go AST fingerprint directory: %w", err)
	}
	file, err := os.Create(output)
	if err != nil {
		return fmt.Errorf("create Go AST fingerprint output: %w", err)
	}
	defer file.Close()
	encoder := json.NewEncoder(file)
	encoder.SetEscapeHTML(false)
	encoder.SetIndent("", "\t")
	oracle := stdlibOracle{
		Schema:         stdlibSchema,
		GoVersion:      runtime.Version(),
		Coordinates:    "raw-utf8-bytes",
		ParseMode:      slices.Clone(parseModeNames),
		ExcludedFields: slices.Clone(excludedFields),
		Sources:        records,
	}
	if err := encoder.Encode(oracle); err != nil {
		return fmt.Errorf("encode Go AST fingerprints: %w", err)
	}
	return nil
}

func goSources(root string) ([]string, error) {
	var sources []string
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() {
			if path != root && entry.Name() == "testdata" {
				return fs.SkipDir
			}
			return nil
		}
		if strings.HasSuffix(entry.Name(), ".go") {
			sources = append(sources, path)
		}
		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("walk Go sources under %s: %w", root, err)
	}
	slices.Sort(sources)
	return sources, nil
}

func parseErrorCount(parseErr error) int {
	if parseErr == nil {
		return 0
	}
	var parseErrors scanner.ErrorList
	if errors.As(parseErr, &parseErrors) {
		return len(parseErrors)
	}
	return 1
}

func parseArguments(arguments []string) (bool, error) {
	if len(arguments) != 1 {
		return false, errors.New("usage: go run . --check|--update")
	}
	switch arguments[0] {
	case "--check":
		return false, nil
	case "--update":
		return true, nil
	default:
		return false, fmt.Errorf("unknown argument %q; expected --check or --update", arguments[0])
	}
}

func buildSnapshot(root string) (snapshot, error) {
	nodeKinds := make(map[string]struct{})
	accepted, err := acceptedCases(filepath.Join(root, "fixtures", "parse-accepted"), nodeKinds)
	if err != nil {
		return snapshot{}, err
	}
	rejected, err := rejectedCases(filepath.Join(root, "fixtures", "parse-rejected"))
	if err != nil {
		return snapshot{}, err
	}
	if len(accepted) == 0 || len(rejected) == 0 {
		return snapshot{}, errors.New("both accepted and rejected Go fixtures are required")
	}

	kinds := make([]string, 0, len(nodeKinds))
	for kind := range nodeKinds {
		kinds = append(kinds, kind)
	}
	slices.Sort(kinds)
	if !slices.Equal(kinds, requiredNodeKinds) {
		missing := difference(requiredNodeKinds, kinds)
		unexpected := difference(kinds, requiredNodeKinds)
		return snapshot{}, fmt.Errorf(
			"accepted fixtures do not cover the owned Go AST surface; missing=%v unexpected=%v",
			missing,
			unexpected,
		)
	}

	return snapshot{
		Schema:                snapshotSchema,
		GoVersion:             runtime.Version(),
		Coordinates:           "raw-utf8-bytes",
		ParseMode:             slices.Clone(parseModeNames),
		ExcludedFields:        slices.Clone(excludedFields),
		RecoveryOnlyNodeKinds: slices.Clone(recoveryOnlyNodeKinds),
		OutOfScopeNodeKinds:   slices.Clone(outOfScopeNodeKinds),
		NodeKinds:             kinds,
		Accepted:              accepted,
		Rejected:              rejected,
	}, nil
}

func acceptedCases(directory string, nodeKinds map[string]struct{}) ([]acceptedCase, error) {
	fixtures, err := readFixtures(directory)
	if err != nil {
		return nil, err
	}
	cases := make([]acceptedCase, 0, len(fixtures))
	for _, fixture := range fixtures {
		files := token.NewFileSet()
		file, parseErr := parser.ParseFile(files, fixture.SourceName, fixture.Source, parseMode)
		if parseErr != nil {
			return nil, fmt.Errorf("%s must be accepted by go/parser: %w", fixture.ID, parseErr)
		}
		if file == nil {
			return nil, fmt.Errorf("%s produced no go/ast.File", fixture.ID)
		}
		projector := normalizer{files: files, nodeKinds: nodeKinds}
		projected, err := projector.node(file)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", fixture.ID, err)
		}
		if projected == nil {
			return nil, fmt.Errorf("%s produced a nil AST projection", fixture.ID)
		}
		cases = append(cases, acceptedCase{
			ID:         fixture.ID,
			SourceName: fixture.SourceName,
			Source:     fixture.Source,
			AST:        *projected,
		})
	}
	return cases, nil
}

func rejectedCases(directory string) ([]rejectedCase, error) {
	fixtures, err := readFixtures(directory)
	if err != nil {
		return nil, err
	}
	cases := make([]rejectedCase, 0, len(fixtures))
	for _, fixture := range fixtures {
		files := token.NewFileSet()
		_, parseErr := parser.ParseFile(files, fixture.SourceName, fixture.Source, parseMode)
		if parseErr == nil {
			return nil, fmt.Errorf("%s must be rejected by go/parser", fixture.ID)
		}
		cases = append(cases, rejectedCase{
			ID:         fixture.ID,
			SourceName: fixture.SourceName,
			Source:     fixture.Source,
		})
	}
	return cases, nil
}

func readFixtures(directory string) ([]fixture, error) {
	root := os.DirFS(directory)
	var fixtures []fixture
	err := fs.WalkDir(root, ".", func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() || !strings.HasSuffix(path, ".go") {
			return nil
		}
		bytes, err := fs.ReadFile(root, path)
		if err != nil {
			return err
		}
		if !utf8.Valid(bytes) {
			return fmt.Errorf("%s is not UTF-8", path)
		}
		id := strings.TrimSuffix(path, ".go")
		fixtures = append(fixtures, fixture{
			ID:         id,
			SourceName: filepath.Base(path),
			Source:     string(bytes),
		})
		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("read fixtures from %s: %w", directory, err)
	}
	return fixtures, nil
}

func (normalizer *normalizer) node(node ast.Node) (*normalizedNode, error) {
	if isNilNode(node) {
		return nil, nil
	}
	value := reflect.ValueOf(node)
	if value.Kind() != reflect.Pointer || value.Elem().Kind() != reflect.Struct {
		return nil, fmt.Errorf("unsupported ast.Node representation %T", node)
	}
	value = value.Elem()
	nodeKind := value.Type().Name()
	normalizer.nodeKinds[nodeKind] = struct{}{}

	fields := make([]normalizedField, 0, value.NumField())
	for index := range value.NumField() {
		field := value.Type().Field(index)
		if field.PkgPath != "" || excludedField(nodeKind, field.Name) {
			continue
		}
		normalized, err := normalizer.value(value.Field(index))
		if err != nil {
			return nil, fmt.Errorf("%s.%s: %w", nodeKind, field.Name, err)
		}
		fields = append(fields, normalizedField{Field: field.Name, Value: normalized})
	}
	from, err := normalizer.position(node.Pos())
	if err != nil {
		return nil, fmt.Errorf("%s.Pos: %w", nodeKind, err)
	}
	to, err := normalizer.position(node.End())
	if err != nil {
		return nil, fmt.Errorf("%s.End: %w", nodeKind, err)
	}
	return &normalizedNode{
		Kind:   nodeKind,
		Start:  from,
		End:    to,
		Fields: fields,
	}, nil
}

func (normalizer *normalizer) value(value reflect.Value) (any, error) {
	if !value.IsValid() {
		return nil, nil
	}
	if value.Type() == tokenPosType {
		return normalizer.position(value.Interface().(token.Pos))
	}
	if value.Type() == tokenType {
		return value.Interface().(token.Token).String(), nil
	}
	if value.Type() == chanDirType {
		return channelDirection(value.Interface().(ast.ChanDir)), nil
	}
	if value.Kind() == reflect.Interface {
		if value.IsNil() {
			return nil, nil
		}
		return normalizer.value(value.Elem())
	}
	if value.Kind() == reflect.Pointer {
		if value.IsNil() {
			return nil, nil
		}
		if value.Type().Implements(astNodeType) {
			return normalizer.node(value.Interface().(ast.Node))
		}
		return normalizer.value(value.Elem())
	}
	if value.CanAddr() && value.Addr().Type().Implements(astNodeType) {
		return normalizer.node(value.Addr().Interface().(ast.Node))
	}

	switch value.Kind() {
	case reflect.Slice, reflect.Array:
		values := make([]any, 0, value.Len())
		for index := range value.Len() {
			normalized, err := normalizer.value(value.Index(index))
			if err != nil {
				return nil, fmt.Errorf("element %d: %w", index, err)
			}
			values = append(values, normalized)
		}
		return values, nil
	case reflect.String:
		return value.String(), nil
	case reflect.Bool:
		return value.Bool(), nil
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return value.Int(), nil
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		return value.Uint(), nil
	case reflect.Struct:
		return nil, fmt.Errorf("unsupported non-node struct %s", value.Type())
	case reflect.Map, reflect.Func, reflect.Chan, reflect.UnsafePointer:
		return nil, fmt.Errorf("unsupported %s value %s", value.Kind(), value.Type())
	default:
		return nil, fmt.Errorf("unsupported value %s", value.Type())
	}
}

func (normalizer *normalizer) position(position token.Pos) (*int, error) {
	if !position.IsValid() {
		return nil, nil
	}
	resolved := normalizer.files.PositionFor(position, false)
	if !resolved.IsValid() {
		return nil, fmt.Errorf("unresolved token.Pos %d", position)
	}
	return &resolved.Offset, nil
}

func (fingerprint *astFingerprint) addNode(node *normalizedNode) error {
	fingerprint.nodes++
	fingerprint.addString(node.Kind)
	fingerprint.addOptionalInt(node.Start)
	fingerprint.addOptionalInt(node.End)
	fingerprint.addLong(uint64(len(node.Fields)))
	for _, field := range node.Fields {
		fingerprint.addString(field.Field)
		if err := fingerprint.addValue(field.Value); err != nil {
			return fmt.Errorf("%s.%s: %w", node.Kind, field.Field, err)
		}
	}
	return nil
}

func (fingerprint *astFingerprint) addValue(value any) error {
	switch value := value.(type) {
	case nil:
		fingerprint.addLong(0)
	case string:
		fingerprint.addLong(1)
		fingerprint.addString(value)
	case bool:
		fingerprint.addLong(2)
		if value {
			fingerprint.addLong(1)
		} else {
			fingerprint.addLong(0)
		}
	case *int:
		if value == nil {
			fingerprint.addLong(0)
		} else {
			fingerprint.addLong(3)
			fingerprint.addLong(uint64(*value))
		}
	case int64:
		fingerprint.addLong(3)
		fingerprint.addLong(uint64(value))
	case uint64:
		fingerprint.addLong(3)
		fingerprint.addLong(value)
	case *normalizedNode:
		if value == nil {
			fingerprint.addLong(0)
		} else {
			fingerprint.addLong(4)
			if err := fingerprint.addNode(value); err != nil {
				return err
			}
		}
	case []any:
		fingerprint.addLong(5)
		fingerprint.addLong(uint64(len(value)))
		for _, element := range value {
			if err := fingerprint.addValue(element); err != nil {
				return err
			}
		}
	default:
		return fmt.Errorf("unsupported normalized value %T", value)
	}
	return nil
}

func (fingerprint *astFingerprint) addOptionalInt(value *int) {
	if value == nil {
		fingerprint.addLong(0)
	} else {
		fingerprint.addLong(1)
		fingerprint.addLong(uint64(*value))
	}
}

func (fingerprint *astFingerprint) addString(value string) {
	bytes := []byte(value)
	fingerprint.addLong(uint64(len(bytes)))
	for _, valueByte := range bytes {
		fingerprint.addLong(uint64(valueByte))
	}
}

func (fingerprint *astFingerprint) addLong(value uint64) {
	fingerprint.state = (fingerprint.state ^ value) * fnvPrime
}

func (fingerprint *astFingerprint) hexadecimal() string {
	return fmt.Sprintf("%016x", fingerprint.state)
}

func excludedField(owner string, field string) bool {
	return slices.Contains(excludedFields, owner+"."+field)
}

func channelDirection(direction ast.ChanDir) string {
	switch direction {
	case ast.SEND:
		return "SEND"
	case ast.RECV:
		return "RECV"
	case ast.SEND | ast.RECV:
		return "SEND|RECV"
	default:
		return fmt.Sprintf("ChanDir(%d)", direction)
	}
}

func isNilNode(node ast.Node) bool {
	if node == nil {
		return true
	}
	value := reflect.ValueOf(node)
	return value.Kind() == reflect.Pointer && value.IsNil()
}

func difference(left []string, right []string) []string {
	var difference []string
	for _, value := range left {
		if !slices.Contains(right, value) {
			difference = append(difference, value)
		}
	}
	return difference
}

func renderSnapshot(snapshot snapshot) ([]byte, error) {
	var output bytes.Buffer
	encoder := json.NewEncoder(&output)
	encoder.SetEscapeHTML(false)
	encoder.SetIndent("", "\t")
	if err := encoder.Encode(snapshot); err != nil {
		return nil, fmt.Errorf("encode snapshot: %w", err)
	}
	return output.Bytes(), nil
}
