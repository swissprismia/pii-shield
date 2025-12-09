#!/usr/bin/env python3
"""
Presidio Sidecar for PII Shield

This script runs as a subprocess managed by the Tauri application.
It receives text via stdin, analyzes it for PII using Microsoft Presidio,
and returns anonymized text via stdout.

Communication Protocol:
- Input: JSON lines via stdin
- Output: JSON lines via stdout
- Format: {"action": "analyze", "text": "..."}
- Response: {"success": true, "anonymized_text": "...", "entities": [...]}

Tokenization Protocol:
- Format: {"action": "tokenize", "text": "...", "entities": [...]}
- Response: {"success": true, "tokenized_text": "...", "token_map": {...}}

De-tokenization Protocol:
- Format: {"action": "detokenize", "text": "...", "token_map": {...}}
- Response: {"success": true, "detokenized_text": "..."}
"""

import json
import sys
import signal
import re
from typing import Optional, Dict, List, Tuple
from collections import defaultdict


def setup_presidio():
    """Initialize Presidio analyzer and anonymizer with spaCy NLP engine."""
    try:
        from presidio_analyzer import AnalyzerEngine
        from presidio_anonymizer import AnonymizerEngine
        from presidio_anonymizer.entities import OperatorConfig

        # Initialize the analyzer with default NLP engine (spaCy)
        analyzer = AnalyzerEngine()

        # Initialize the anonymizer
        anonymizer = AnonymizerEngine()

        return analyzer, anonymizer
    except ImportError as e:
        log_error(f"Failed to import Presidio: {e}")
        return None, None


def log_error(message: str):
    """Log error messages to stderr."""
    print(f"[ERROR] {message}", file=sys.stderr, flush=True)


def log_info(message: str):
    """Log info messages to stderr."""
    print(f"[INFO] {message}", file=sys.stderr, flush=True)


def analyze_text(
    analyzer, anonymizer, text: str, language: str = "en"
) -> dict:
    """
    Analyze text for PII and return anonymized version.

    Args:
        analyzer: Presidio AnalyzerEngine instance
        anonymizer: Presidio AnonymizerEngine instance
        text: Text to analyze
        language: Language code (default: "en")

    Returns:
        Dictionary with analysis results
    """
    try:
        # Analyze the text
        results = analyzer.analyze(
            text=text,
            language=language,
            entities=None,  # Use all supported entities
            score_threshold=0.5,  # Confidence threshold
        )

        if not results:
            return {
                "success": True,
                "anonymized_text": text,
                "entities": [],
            }

        # Create entity list for response
        entities = []
        for result in results:
            entities.append({
                "entity_type": result.entity_type,
                "text": text[result.start:result.end],
                "start": result.start,
                "end": result.end,
                "score": result.score,
            })

        # Anonymize the text
        anonymized = anonymizer.anonymize(
            text=text,
            analyzer_results=results,
        )

        return {
            "success": True,
            "anonymized_text": anonymized.text,
            "entities": entities,
        }

    except Exception as e:
        log_error(f"Analysis failed: {e}")
        return {
            "success": False,
            "error": str(e),
            "anonymized_text": text,
            "entities": [],
        }


def fallback_analyze(text: str) -> dict:
    """
    Fallback analysis using regex patterns when Presidio is not available.
    """
    import re

    patterns = {
        "EMAIL_ADDRESS": (
            r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
            "[EMAIL]"
        ),
        "PHONE_NUMBER": (
            r"\b(\+?1[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b",
            "[PHONE]"
        ),
        "CREDIT_CARD": (
            r"\b\d{4}[-\s]?\d{4}[-\s]?\d{4}[-\s]?\d{4}\b",
            "[CREDIT_CARD]"
        ),
        "US_SSN": (
            r"\b\d{3}[-\s]?\d{2}[-\s]?\d{4}\b",
            "[SSN]"
        ),
        "IP_ADDRESS": (
            r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b",
            "[IP_ADDRESS]"
        ),
        "URL": (
            r"https?://[^\s<>\"{}|\\^`\[\]]+",
            "[URL]"
        ),
    }

    entities = []
    anonymized = text

    # Collect all matches
    all_matches = []
    for entity_type, (pattern, replacement) in patterns.items():
        for match in re.finditer(pattern, text):
            all_matches.append({
                "entity_type": entity_type,
                "text": match.group(),
                "start": match.start(),
                "end": match.end(),
                "score": 0.85,
                "replacement": replacement,
            })

    # Sort by position (reverse) for replacement
    all_matches.sort(key=lambda x: x["start"], reverse=True)

    # Build entities list and anonymized text
    for match in all_matches:
        entities.append({
            "entity_type": match["entity_type"],
            "text": match["text"],
            "start": match["start"],
            "end": match["end"],
            "score": match["score"],
        })
        anonymized = (
            anonymized[:match["start"]]
            + match["replacement"]
            + anonymized[match["end"]:]
        )

    # Reverse to match original order
    entities.reverse()

    return {
        "success": True,
        "anonymized_text": anonymized,
        "entities": entities,
    }


# Entity type to human-readable token prefix mapping
ENTITY_TOKEN_PREFIXES = {
    "PERSON": ["FirstName", "LastName", "Name"],
    "EMAIL_ADDRESS": ["Email"],
    "PHONE_NUMBER": ["Phone"],
    "CREDIT_CARD": ["CreditCard"],
    "US_SSN": ["SSN"],
    "IP_ADDRESS": ["IP"],
    "URL": ["URL"],
    "LOCATION": ["Location"],
    "DATE_TIME": ["Date"],
    "DOMAIN_NAME": ["Domain"],
    "IBAN_CODE": ["IBAN"],
    "US_BANK_NUMBER": ["BankAccount"],
    "US_PASSPORT": ["Passport"],
    "NRP": ["NRP"],
    "MEDICAL_LICENSE": ["MedicalLicense"],
}


def split_person_name(text: str) -> List[Tuple[str, str]]:
    """
    Split a person's name into first name and last name components.
    Returns list of (token_prefix, value) tuples.
    """
    parts = text.strip().split()
    if len(parts) == 0:
        return [("Name", text)]
    elif len(parts) == 1:
        return [("FirstName", parts[0])]
    elif len(parts) == 2:
        return [("FirstName", parts[0]), ("LastName", parts[1])]
    else:
        # More than 2 parts: first is first name, last is last name, middle parts are middle names
        result = [("FirstName", parts[0])]
        for i, part in enumerate(parts[1:-1], 1):
            result.append((f"MiddleName", part))
        result.append(("LastName", parts[-1]))
        return result


def tokenize_text(text: str, entities: List[dict]) -> dict:
    """
    Tokenize PII entities in text with human-readable tokens.

    Example:
        Input: "John Doe's email is john.doe@example.com"
        Output: "[FirstName1] [LastName1]'s email is [Email1]"
        Token map: {"FirstName1": "John", "LastName1": "Doe", "Email1": "john.doe@example.com"}
    """
    if not entities:
        return {
            "success": True,
            "tokenized_text": text,
            "token_map": {},
        }

    # Track token counters by prefix
    token_counters: Dict[str, int] = defaultdict(int)

    # Build token mappings
    token_map: Dict[str, str] = {}

    # Sort entities by start position (ascending) to process in order
    # but we'll build replacements first, then apply from end to start
    sorted_entities = sorted(entities, key=lambda x: x["start"])

    # Build replacements list
    replacements: List[Tuple[int, int, str]] = []

    for entity in sorted_entities:
        entity_type = entity["entity_type"]
        original_value = entity["text"]
        start = entity["start"]
        end = entity["end"]

        if entity_type == "PERSON":
            # Split person names into first/last name tokens
            name_parts = split_person_name(original_value)
            token_parts = []

            for prefix, value in name_parts:
                token_counters[prefix] += 1
                token_id = f"{prefix}{token_counters[prefix]}"
                token_map[token_id] = value
                token_parts.append(f"[{token_id}]")

            replacement = " ".join(token_parts)
            replacements.append((start, end, replacement))
        else:
            # Get the token prefix for this entity type
            prefixes = ENTITY_TOKEN_PREFIXES.get(entity_type, [entity_type])
            prefix = prefixes[0]

            token_counters[prefix] += 1
            token_id = f"{prefix}{token_counters[prefix]}"
            token_map[token_id] = original_value

            replacement = f"[{token_id}]"
            replacements.append((start, end, replacement))

    # Apply replacements from end to start (reverse order)
    # This ensures positions remain valid as we replace
    tokenized = text
    for start, end, replacement in reversed(replacements):
        tokenized = tokenized[:start] + replacement + tokenized[end:]

    return {
        "success": True,
        "tokenized_text": tokenized,
        "token_map": token_map,
    }


def detokenize_text(text: str, token_map: Dict[str, str]) -> dict:
    """
    De-tokenize text by replacing tokens with original values.

    Example:
        Input: "[FirstName1] [LastName1] is a great developer"
        Token map: {"FirstName1": "John", "LastName1": "Doe"}
        Output: "John Doe is a great developer"
    """
    if not token_map:
        return {
            "success": True,
            "detokenized_text": text,
        }

    detokenized = text

    # Find and replace all tokens in the text
    # Token pattern: [TokenName1], [FirstName1], etc.
    token_pattern = re.compile(r'\[([A-Za-z]+\d+)\]')

    def replace_token(match):
        token_id = match.group(1)
        if token_id in token_map:
            return token_map[token_id]
        return match.group(0)  # Keep original if not found

    detokenized = token_pattern.sub(replace_token, text)

    return {
        "success": True,
        "detokenized_text": detokenized,
    }


def detect_tokens_in_text(text: str) -> List[str]:
    """
    Detect token patterns in text.
    Returns list of token IDs found in the text.
    """
    token_pattern = re.compile(r'\[([A-Za-z]+\d+)\]')
    matches = token_pattern.findall(text)
    return matches


def handle_request(
    request: dict,
    analyzer,
    anonymizer,
    use_fallback: bool = False
) -> dict:
    """Handle a single request from the Tauri app."""
    action = request.get("action", "")

    if action == "analyze":
        text = request.get("text", "")
        if not text:
            return {"success": True, "anonymized_text": "", "entities": []}

        if use_fallback or analyzer is None:
            return fallback_analyze(text)
        else:
            return analyze_text(analyzer, anonymizer, text)

    elif action == "tokenize":
        text = request.get("text", "")
        entities = request.get("entities", [])

        if not text:
            return {"success": True, "tokenized_text": "", "token_map": {}}

        return tokenize_text(text, entities)

    elif action == "detokenize":
        text = request.get("text", "")
        token_map = request.get("token_map", {})

        if not text:
            return {"success": True, "detokenized_text": ""}

        return detokenize_text(text, token_map)

    elif action == "detect_tokens":
        text = request.get("text", "")
        tokens = detect_tokens_in_text(text)
        return {
            "success": True,
            "tokens": tokens,
            "has_tokens": len(tokens) > 0,
        }

    elif action == "analyze_and_tokenize":
        # Combined action: analyze for PII and tokenize in one step
        text = request.get("text", "")
        if not text:
            return {
                "success": True,
                "original_text": text,
                "tokenized_text": "",
                "token_map": {},
                "entities": [],
            }

        # First analyze
        if use_fallback or analyzer is None:
            analysis = fallback_analyze(text)
        else:
            analysis = analyze_text(analyzer, anonymizer, text)

        if not analysis.get("success"):
            return analysis

        # Then tokenize using the detected entities
        entities = analysis.get("entities", [])
        tokenization = tokenize_text(text, entities)

        return {
            "success": True,
            "original_text": text,
            "tokenized_text": tokenization.get("tokenized_text", text),
            "token_map": tokenization.get("token_map", {}),
            "entities": entities,
        }

    elif action == "ping":
        return {"success": True, "message": "pong"}

    elif action == "status":
        return {
            "success": True,
            "presidio_available": analyzer is not None,
            "version": "0.2.0",
        }

    else:
        return {"success": False, "error": f"Unknown action: {action}"}


def main():
    """Main entry point for the sidecar."""
    # Handle SIGTERM gracefully
    def signal_handler(signum, frame):
        log_info("Received shutdown signal")
        sys.exit(0)

    signal.signal(signal.SIGTERM, signal_handler)
    signal.signal(signal.SIGINT, signal_handler)

    # Try to initialize Presidio
    log_info("Initializing Presidio sidecar...")
    analyzer, anonymizer = setup_presidio()

    use_fallback = analyzer is None
    if use_fallback:
        log_info("Presidio not available, using fallback regex patterns")
    else:
        log_info("Presidio initialized successfully")

    # Signal ready
    print(json.dumps({"status": "ready", "presidio": not use_fallback}), flush=True)

    # Main loop: read requests from stdin, write responses to stdout
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            request = json.loads(line)
            response = handle_request(request, analyzer, anonymizer, use_fallback)
            print(json.dumps(response), flush=True)
        except json.JSONDecodeError as e:
            log_error(f"Invalid JSON: {e}")
            print(json.dumps({"success": False, "error": "Invalid JSON"}), flush=True)
        except Exception as e:
            log_error(f"Error handling request: {e}")
            print(json.dumps({"success": False, "error": str(e)}), flush=True)


if __name__ == "__main__":
    main()
