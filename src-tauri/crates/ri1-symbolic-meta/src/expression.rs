use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Symbol(char),
    Operator(char),
    LParen,
    RParen,
    Caret,
    Slash,
    Plus,
    Minus,
    Pipe,
    Colon,
    Arrow,
    Equals,
    LBracket,
    RBracket,
    Whitespace,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub position: usize,
}

#[derive(Debug, Clone)]
pub struct ExpressionGroup {
    pub id: String,
    pub content: String,
    pub start: usize,
    pub end: usize,
    pub depth: usize,
    pub parent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedExpression {
    pub original: String,
    pub tokens: Vec<Token>,
    pub groups: Vec<ExpressionGroup>,
    pub symbols: HashMap<char, String>,
    pub operators: Vec<(char, String)>,
    pub unresolved: Vec<String>,
}

pub struct ExpressionParser;

impl ExpressionParser {
    pub fn parse(input: &str) -> ParsedExpression {
        let tokens = Self::tokenize(input);
        let groups = Self::extract_groups(input, &tokens);
        let symbols = Self::extract_symbols(&tokens);
        let operators = Self::extract_operators(&tokens);
        let unresolved = Self::identify_unresolved(&tokens, &groups);

        ParsedExpression {
            original: input.to_string(),
            tokens,
            groups,
            symbols,
            operators,
            unresolved,
        }
    }

    fn tokenize(input: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut pos = 0;
        let chars: Vec<char> = input.chars().collect();

        while pos < chars.len() {
            let ch = chars[pos];
            let kind = match ch {
                '(' => TokenKind::LParen,
                ')' => TokenKind::RParen,
                '^' => TokenKind::Caret,
                '/' => TokenKind::Slash,
                '+' => TokenKind::Plus,
                '-' => TokenKind::Minus,
                '|' => TokenKind::Pipe,
                ':' => TokenKind::Colon,
                '→' => TokenKind::Arrow,
                '=' => TokenKind::Equals,
                '[' => TokenKind::LBracket,
                ']' => TokenKind::RBracket,
                ' ' | '\t' | '\n' | '\r' => TokenKind::Whitespace,
                _ => {
                    if ch.is_alphabetic() || ch.is_numeric() || ch == 'n' {
                        TokenKind::Symbol(ch)
                    } else {
                        TokenKind::Operator(ch)
                    }
                }
            };

            tokens.push(Token {
                kind,
                text: ch.to_string(),
                position: pos, // Use character index, not byte index
            });
            pos += 1;
        }

        tokens
    }

    fn extract_groups(input: &str, tokens: &[Token]) -> Vec<ExpressionGroup> {
        let mut groups = Vec::new();
        let mut stack: Vec<(usize, usize)> = Vec::new(); // (start_char_pos, depth)
        let mut group_id = 0;
        let chars: Vec<char> = input.chars().collect();

        for token in tokens {
            match token.kind {
                TokenKind::LParen => {
                    stack.push((token.position, stack.len()));
                }
                TokenKind::RParen => {
                    if let Some((start, depth)) = stack.pop() {
                        let end = token.position;
                        // Extract content using character indices
                        let content: String = chars[start..=end].iter().collect();
                        groups.push(ExpressionGroup {
                            id: format!("G{}", group_id),
                            content,
                            start,
                            end,
                            depth,
                            parent: None,
                        });
                        group_id += 1;
                    }
                }
                _ => {}
            }
        }

        // Set parent relationships
        for i in 0..groups.len() {
            for j in 0..groups.len() {
                if i != j && groups[i].start > groups[j].start && groups[i].end < groups[j].end {
                    groups[i].parent = Some(groups[j].id.clone());
                    break;
                }
            }
        }

        groups
    }

    fn extract_symbols(tokens: &[Token]) -> HashMap<char, String> {
        let mut symbols = HashMap::new();
        let known_symbols = [
            ('Φ', "harmonic equilibrium - preserves tension, non-fusional, non-neutralizing, recursion-safe"),
            ('π', "recursive continuation - maintains pattern fidelity under infinite extension"),
            ('ε', "incremental adjustment - micro-ignition, local activation within active recursion"),
            ('Ρ', "perceptual modulation - alters meaning trajectory via frame without breaking identity"),
            ('Τ', "synchronization/readiness - enables transformation readiness without forcing it"),
            ('Θ', "intentional configuration - sets directed recursive potential (field intention)"),
            ('n', "recursion-depth index - index/depth/count modifier"),
            ('Λ', "structural illumination - renders structural clarity from recursive fields"),
            ('Δ', "fusion transformation - fusion collapse, irreversible transformation"),
            ('Ω', "closure integration - closes active recursion and fixes structure"),
            ('Ψ', "oscillation - modulation of recursive tempo/pressure"),
            ('Γ', "directional growth - forward recursion with identity continuity"),
            ('Σ', "coexistence plurality - plural recursion without fusion"),
            ('Ξ', "emergent system - emergent systemic coherence from recursive density"),
            ('ζ', "recurrence pattern - meaningful reappearance of motifs"),
            ('λ', "entanglement - non-local dependency across recursion threads"),
            ('ω', "will force - internal will vector biases recursive direction"),
            ('δ', "micro transformation - fine-grain recursion adjustment / perturbation"),
            ('χ', "measurement perception bridge - harmonic tuning; collapse to perceivable state"),
        ];

        for token in tokens {
            if let TokenKind::Symbol(ch) = token.kind {
                if let Some((_, desc)) = known_symbols.iter().find(|(c, _)| *c == ch) {
                    symbols.insert(ch, desc.to_string());
                }
            }
        }

        symbols
    }

    fn extract_operators(tokens: &[Token]) -> Vec<(char, String)> {
        let mut operators = Vec::new();
        let known_operators = [
            ('/', "disruption/interference analysis - rupture, interference, non-interacting fields"),
            ('+', "parallel coexistence - coexistent recursion states, simultaneity"),
            ('-', "subtraction - removal, difference, negation"),
            ('^', "exponentiation - power, repeated application, scaling"),
            ('→', "flow vector - directed recursion motion, causal flow"),
            (':', "interaction interface - relational interface / tension-contact"),
            ('|', "orthogonality - non-interacting fields, parallel independence"),
            ('=', "stabilization resolution - final form / resolution, terminal state"),
        ];

        for token in tokens {
            let op_char = match token.kind {
                TokenKind::Slash => Some('/'),
                TokenKind::Plus => Some('+'),
                TokenKind::Minus => Some('-'),
                TokenKind::Caret => Some('^'),
                TokenKind::Arrow => Some('→'),
                TokenKind::Colon => Some(':'),
                TokenKind::Pipe => Some('|'),
                TokenKind::Equals => Some('='),
                _ => None,
            };

            if let Some(ch) = op_char {
                if let Some((_, desc)) = known_operators.iter().find(|(c, _)| *c == ch) {
                    operators.push((ch, desc.to_string()));
                }
            }
        }

        operators
    }

    fn identify_unresolved(tokens: &[Token], groups: &[ExpressionGroup]) -> Vec<String> {
        let mut unresolved = Vec::new();

        // Check for juxtaposition without explicit operator
        for i in 0..tokens.len().saturating_sub(1) {
            let curr = &tokens[i];
            let next = &tokens[i + 1];

            if matches!(curr.kind, TokenKind::Symbol(_)) && matches!(next.kind, TokenKind::Symbol(_)) {
                // Check if there's no operator between them
                let has_operator = tokens[i..=i+1].iter().any(|t| {
                    matches!(t.kind, TokenKind::Slash | TokenKind::Plus | TokenKind::Minus | TokenKind::Caret | TokenKind::Arrow | TokenKind::Colon | TokenKind::Pipe | TokenKind::Equals)
                });

                if !has_operator {
                    unresolved.push(format!("juxtaposition of '{}' and '{}' at position {}-{}", curr.text, next.text, curr.position, next.position));
                }
            }
        }

        // Check for ambiguous slash binding
        let slash_count = tokens.iter().filter(|t| matches!(t.kind, TokenKind::Slash)).count();
        if slash_count > 1 {
            unresolved.push(format!("ambiguous slash binding ({} occurrences) - binding precedence unclear", slash_count));
        }

        // Check for exponentiation by non-numeric
        for i in 0..tokens.len().saturating_sub(1) {
            if matches!(tokens[i].kind, TokenKind::Caret) {
                if let Some(next) = tokens.get(i + 1) {
                    if matches!(next.kind, TokenKind::Symbol(_)) {
                        unresolved.push(format!("exponentiation by symbol '{}' - non-standard exponent", next.text));
                    }
                }
            }
        }

        // Check for specific RI1 patterns
        let token_texts: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        
        // Check for n-Θ pattern (subtraction with index)
        if token_texts.windows(3).any(|w| w == ["n", "-", "Θ"]) {
            unresolved.push("pattern 'n-Θ' - index subtraction, semantics unclear".to_string());
        }
        
        // Check for Θn pattern (juxtaposition with index)
        if token_texts.windows(2).any(|w| w == ["Θ", "n"]) {
            unresolved.push("pattern 'Θn' - intention-index juxtaposition, binding unclear".to_string());
        }
        
        // Check for Τ(n-Θ) pattern (function application)
        if token_texts.windows(5).any(|w| w == ["Τ", "(", "n", "-", "Θ"]) {
            unresolved.push("pattern 'Τ(n-Θ)' - synchronization applied to index difference, composition unclear".to_string());
        }
        
        // Check for Φπε juxtaposition
        if token_texts.windows(3).any(|w| w == ["Φ", "π", "ε"]) {
            unresolved.push("pattern 'Φπε' - triple symbol juxtaposition, relationship unclear".to_string());
        }

        // Check for nested parentheses complexity
        let max_depth = groups.iter().map(|g| g.depth).max().unwrap_or(0);
        if max_depth > 2 {
            unresolved.push(format!("deep nesting (depth {}) - complex hierarchical structure", max_depth));
        }

        unresolved
    }

    pub fn validate_expression(parsed: &ParsedExpression) -> Vec<String> {
        let mut issues = Vec::new();

        // Check for balanced parentheses
        let mut balance = 0;
        for token in &parsed.tokens {
            match token.kind {
                TokenKind::LParen => balance += 1,
                TokenKind::RParen => balance -= 1,
                _ => {}
            }
            if balance < 0 {
                issues.push("unbalanced closing parenthesis".to_string());
                break;
            }
        }
        if balance != 0 {
            issues.push("unbalanced opening parenthesis".to_string());
        }

        // Check for unknown symbols
        let known_greek = ['Φ', 'π', 'ε', 'Ρ', 'Τ', 'Θ', 'Λ', 'Δ', 'Ω', 'Ψ', 'Γ', 'Σ', 'Ξ', 'ζ', 'λ', 'ω', 'δ', 'χ'];
        for token in &parsed.tokens {
            if let TokenKind::Symbol(ch) = token.kind {
                if ch.is_alphabetic() && !known_greek.contains(&ch) && ch != 'n' {
                    issues.push(format!("unknown symbol '{}'", ch));
                }
            }
        }

        // Check for missing operators
        if parsed.operators.is_empty() && parsed.tokens.len() > 1 {
            issues.push("no operators found".to_string());
        }

        // Check for consecutive operators
        for i in 0..parsed.tokens.len().saturating_sub(1) {
            let curr = &parsed.tokens[i];
            let next = &parsed.tokens[i + 1];
            if matches!(curr.kind, TokenKind::Slash | TokenKind::Plus | TokenKind::Minus | TokenKind::Caret | TokenKind::Arrow | TokenKind::Colon | TokenKind::Pipe | TokenKind::Equals) &&
               matches!(next.kind, TokenKind::Slash | TokenKind::Plus | TokenKind::Minus | TokenKind::Caret | TokenKind::Arrow | TokenKind::Colon | TokenKind::Pipe | TokenKind::Equals) {
                issues.push(format!("consecutive operators '{}' and '{}'", curr.text, next.text));
            }
        }

        // Check for empty parentheses
        for group in &parsed.groups {
            if group.content.len() <= 2 {
                issues.push(format!("empty or minimal group '{}'", group.content));
            }
        }

        // Check for operator at start/end
        if let Some(first) = parsed.tokens.first() {
            if matches!(first.kind, TokenKind::Slash | TokenKind::Plus | TokenKind::Minus | TokenKind::Caret | TokenKind::Arrow | TokenKind::Colon | TokenKind::Pipe | TokenKind::Equals) {
                issues.push(format!("expression starts with operator '{}'", first.text));
            }
        }
        if let Some(last) = parsed.tokens.last() {
            if matches!(last.kind, TokenKind::Slash | TokenKind::Plus | TokenKind::Minus | TokenKind::Caret | TokenKind::Arrow | TokenKind::Colon | TokenKind::Pipe | TokenKind::Equals) {
                issues.push(format!("expression ends with operator '{}'", last.text));
            }
        }

        issues
    }

    pub fn generate_prompt_context(parsed: &ParsedExpression) -> String {
        let mut context = String::new();

        context.push_str("EXPRESSION ANALYSIS CONTEXT\n");
        context.push_str(&format!("Original: {}\n", parsed.original));
        context.push_str(&format!("Tokens: {}\n", parsed.tokens.len()));
        context.push_str(&format!("Groups: {}\n", parsed.groups.len()));

        if !parsed.groups.is_empty() {
            context.push_str("\nPARENTHESIZED GROUPS:\n");
            for group in &parsed.groups {
                context.push_str(&format!("  {}: {} (depth {}, parent: {})\n", 
                    group.id, group.content, group.depth, 
                    group.parent.as_deref().unwrap_or("none")));
            }
        }

        if !parsed.symbols.is_empty() {
            context.push_str("\nSYMBOLS (RI1 Protocol Definitions):\n");
            for (ch, desc) in &parsed.symbols {
                context.push_str(&format!("  {} = {}\n", ch, desc));
            }
        }

        if !parsed.operators.is_empty() {
            context.push_str("\nOPERATORS (RI1 Protocol Definitions):\n");
            for (ch, desc) in &parsed.operators {
                context.push_str(&format!("  {} = {}\n", ch, desc));
            }
        }

        if !parsed.unresolved.is_empty() {
            context.push_str("\nUNRESOLVED STRUCTURES:\n");
            for issue in &parsed.unresolved {
                context.push_str(&format!("  - {}\n", issue));
            }
        }

        context.push_str("\nINSTRUCTIONS:\n");
        context.push_str("1. Preserve the original expression exactly as written\n");
        context.push_str("2. Do not invent symbols or operators not present in the original\n");
        context.push_str("3. Identify all parenthesized groups and their relationships\n");
        context.push_str("4. Explain the meaning of each symbol based on RI1 protocol\n");
        context.push_str("5. Note any unresolved or ambiguous structures\n");
        context.push_str("6. Do not perform arithmetic or numerical evaluation\n");
        context.push_str("7. Distinguish between lexical structure, documented meanings, and interpretive assumptions\n");
        context.push_str("8. Flag any compositions that are not explicitly defined in the RI1 specification\n");

        context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_expression() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);

        assert_eq!(parsed.original, input);
        assert!(!parsed.tokens.is_empty());
        assert!(!parsed.groups.is_empty());
        assert!(!parsed.symbols.is_empty());
        assert!(!parsed.operators.is_empty());
    }

    #[test]
    fn test_extract_groups() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);

        // Should find 3 groups: (Φπε), (n-Θ), (Θn+Τ/Ρ)
        assert_eq!(parsed.groups.len(), 3);

        let group_contents: Vec<&str> = parsed.groups.iter().map(|g| g.content.as_str()).collect();
        assert!(group_contents.contains(&"(Φπε)"));
        assert!(group_contents.contains(&"(n-Θ)"));
        assert!(group_contents.contains(&"(Θn+Τ/Ρ)"));
    }

    #[test]
    fn test_extract_symbols() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);

        assert!(parsed.symbols.contains_key(&'Φ'));
        assert!(parsed.symbols.contains_key(&'π'));
        assert!(parsed.symbols.contains_key(&'ε'));
        assert!(parsed.symbols.contains_key(&'Ρ'));
        assert!(parsed.symbols.contains_key(&'Τ'));
        assert!(parsed.symbols.contains_key(&'Θ'));
        assert!(parsed.symbols.contains_key(&'n'));
    }

    #[test]
    fn test_extract_operators() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);

        let op_chars: Vec<char> = parsed.operators.iter().map(|(c, _)| *c).collect();
        assert!(op_chars.contains(&'^'));
        assert!(op_chars.contains(&'/'));
        assert!(op_chars.contains(&'-'));
        assert!(op_chars.contains(&'+'));
    }

    #[test]
    fn test_unresolved_structures() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);

        // Should identify unresolved structures
        assert!(!parsed.unresolved.is_empty());
    }

    #[test]
    fn test_validate_expression() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);
        let issues = ExpressionParser::validate_expression(&parsed);

        // Should not have critical issues
        assert!(issues.is_empty() || issues.iter().all(|i| !i.contains("unbalanced")));
    }

    #[test]
    fn test_malformed_parentheses() {
        let input = "(Φπε^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);
        let issues = ExpressionParser::validate_expression(&parsed);

        assert!(issues.iter().any(|i| i.contains("unbalanced")));
    }

    #[test]
    fn test_unknown_symbols() {
        let input = "(ΦπεX)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);
        let issues = ExpressionParser::validate_expression(&parsed);

        assert!(issues.iter().any(|i| i.contains("unknown symbol")));
    }

    #[test]
    fn test_greek_latin_lookalikes() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);

        // Ensure Greek symbols are preserved
        assert!(parsed.symbols.contains_key(&'Φ')); // Greek Phi
        assert!(parsed.symbols.contains_key(&'π')); // Greek pi
        assert!(parsed.symbols.contains_key(&'ε')); // Greek epsilon

        // Ensure Latin lookalikes are not confused
        assert!(!parsed.symbols.contains_key(&'P')); // Latin P
        assert!(!parsed.symbols.contains_key(&'p')); // Latin p
    }

    #[test]
    fn test_specific_ri1_patterns() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);

        // Should identify specific RI1 patterns
        assert!(parsed.unresolved.iter().any(|u| u.contains("Φπε")));
        assert!(parsed.unresolved.iter().any(|u| u.contains("n-Θ")));
        assert!(parsed.unresolved.iter().any(|u| u.contains("Θn")));
        assert!(parsed.unresolved.iter().any(|u| u.contains("Τ(n-Θ)")));
        assert!(parsed.unresolved.iter().any(|u| u.contains("ambiguous slash")));
    }

    #[test]
    fn test_consecutive_operators() {
        let input = "(Φπε)++^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);
        let issues = ExpressionParser::validate_expression(&parsed);

        assert!(issues.iter().any(|i| i.contains("consecutive operators")));
    }

    #[test]
    fn test_empty_parentheses() {
        let input = "(Φπε)^Ρ/Τ()(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);
        let issues = ExpressionParser::validate_expression(&parsed);

        assert!(issues.iter().any(|i| i.contains("empty or minimal group")));
    }

    #[test]
    fn test_operator_at_start() {
        let input = "+(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);
        let issues = ExpressionParser::validate_expression(&parsed);

        assert!(issues.iter().any(|i| i.contains("starts with operator")));
    }

    #[test]
    fn test_operator_at_end() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)+";
        let parsed = ExpressionParser::parse(input);
        let issues = ExpressionParser::validate_expression(&parsed);

        assert!(issues.iter().any(|i| i.contains("ends with operator")));
    }

    #[test]
    fn test_prompt_context_generation() {
        let input = "(Φπε)^Ρ/Τ(n-Θ)/(Θn+Τ/Ρ)";
        let parsed = ExpressionParser::parse(input);
        let context = ExpressionParser::generate_prompt_context(&parsed);

        assert!(context.contains("EXPRESSION ANALYSIS CONTEXT"));
        assert!(context.contains("Original:"));
        assert!(context.contains("PARENTHESIZED GROUPS:"));
        assert!(context.contains("SYMBOLS (RI1 Protocol Definitions):"));
        assert!(context.contains("OPERATORS (RI1 Protocol Definitions):"));
        assert!(context.contains("UNRESOLVED STRUCTURES:"));
        assert!(context.contains("INSTRUCTIONS:"));
    }
}
