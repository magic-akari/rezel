import process from "node:process";
import ts from "typescript";

type InventoryMember = ts.ClassElement | ts.TypeElement | ts.EnumMember;

interface Inventory {
	symbols: string[];
}

function usage(): void {
	process.stdout.write(
		[
			"Usage:",
			"  node enumerate-upstream.ts <source>",
			"",
			"Read TypeScript source from stdin and write its declaration inventory to stdout.",
			"",
		].join("\n"),
	);
}

async function main(): Promise<void> {
	const arguments_ = process.argv.slice(2);
	if (arguments_.length === 1 && ["-h", "--help"].includes(arguments_[0])) {
		usage();
		return;
	}
	if (arguments_.length !== 1) {
		usage();
		throw new Error("expected one source path");
	}

	const [source] = arguments_;
	process.stdin.setEncoding("utf8");
	let text = "";
	for await (const chunk of process.stdin) {
		text += chunk;
	}
	const diagnostics =
		ts.transpileModule(text, {
			compilerOptions: {
				target: ts.ScriptTarget.Latest,
			},
			fileName: source,
			reportDiagnostics: true,
		}).diagnostics ?? [];
	const file = ts.createSourceFile(source, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
	if (diagnostics.length > 0) {
		const messages = diagnostics.map((diagnostic) => {
			const message = ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n");
			if (diagnostic.start === undefined) {
				return message;
			}
			const position = file.getLineAndCharacterOfPosition(diagnostic.start);
			return `${source}:${position.line + 1}:${position.character + 1}: ${message}`;
		});
		throw new Error(messages.join("\n"));
	}

	const symbols = new Set<string>();

	function addName(name: ts.DeclarationName | undefined, owner: string | null = null): void {
		if (!name) {
			return;
		}
		let value: string | undefined;
		if (
			ts.isIdentifier(name) ||
			ts.isPrivateIdentifier(name) ||
			ts.isStringLiteral(name) ||
			ts.isNumericLiteral(name) ||
			ts.isNoSubstitutionTemplateLiteral(name) ||
			ts.isBigIntLiteral(name)
		) {
			value = name.text;
		} else if (ts.isComputedPropertyName(name)) {
			value = name.getText(file);
		}
		if (value) {
			symbols.add(owner ? `${owner}.${value}` : value);
		}
	}

	function addBindingName(name: ts.BindingName, owner: string | null = null): void {
		if (ts.isIdentifier(name)) {
			addName(name, owner);
			return;
		}
		for (const element of name.elements) {
			if (ts.isBindingElement(element)) {
				addBindingName(element.name, owner);
			}
		}
	}

	function addMembers(owner: string, members: readonly InventoryMember[]): void {
		for (const member of members) {
			if (ts.isConstructorDeclaration(member)) {
				symbols.add(`${owner}.constructor`);
			} else if ("name" in member) {
				addName(member.name as ts.DeclarationName | undefined, owner);
			}
		}
	}

	function addStatements(statements: readonly ts.Statement[], namespace: string | null = null): void {
		for (const statement of statements) {
			if (ts.isClassDeclaration(statement) || ts.isInterfaceDeclaration(statement)) {
				if (statement.name) {
					const owner = namespace ? `${namespace}.${statement.name.text}` : statement.name.text;
					symbols.add(owner);
					addMembers(owner, statement.members);
				}
			} else if (ts.isFunctionDeclaration(statement)) {
				addName(statement.name, namespace);
			} else if (ts.isTypeAliasDeclaration(statement)) {
				const owner = namespace ? `${namespace}.${statement.name.text}` : statement.name.text;
				symbols.add(owner);
				if (ts.isTypeLiteralNode(statement.type)) {
					addMembers(owner, statement.type.members);
				}
			} else if (ts.isEnumDeclaration(statement)) {
				const owner = namespace ? `${namespace}.${statement.name.text}` : statement.name.text;
				symbols.add(owner);
				addMembers(owner, statement.members);
			} else if (ts.isVariableStatement(statement)) {
				for (const declaration of statement.declarationList.declarations) {
					addBindingName(declaration.name, namespace);
				}
			} else if (ts.isModuleDeclaration(statement)) {
				const owner = namespace ? `${namespace}.${statement.name.text}` : statement.name.text;
				symbols.add(owner);
				let body = statement.body;
				while (body && ts.isModuleDeclaration(body)) {
					addName(body.name, owner);
					body = body.body;
				}
				if (body && ts.isModuleBlock(body)) {
					addStatements(body.statements, owner);
				}
			}
		}
	}

	addStatements(file.statements);
	const result: Inventory = {
		symbols: [...symbols].sort(),
	};
	process.stdout.write(`${JSON.stringify(result)}\n`);
}

await main().catch((error: unknown) => {
	const message = error instanceof Error ? error.message : String(error);
	process.stderr.write(`${message}\n`);
	process.exitCode = 1;
});
