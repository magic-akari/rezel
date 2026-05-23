package main

import (
	"bytes"
	"go/format"
	"io/fs"
	"os"
	"testing"
)

func TestMaintainedGoSourcesAreFormatted(t *testing.T) {
	t.Parallel()

	paths := []string{"main.go", "main_test.go"}
	err := fs.WalkDir(os.DirFS("fixtures/parse-accepted"), ".", func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if !entry.IsDir() && entry.Type().IsRegular() {
			paths = append(paths, "fixtures/parse-accepted/"+path)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}

	for _, path := range paths {
		source, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		formatted, err := format.Source(source)
		if err != nil {
			t.Fatalf("%s is not valid Go: %v", path, err)
		}
		if !bytes.Equal(source, formatted) {
			t.Errorf("%s is not gofmt-formatted", path)
		}
	}
}
