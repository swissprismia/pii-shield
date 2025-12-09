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
"""

import json
import sys
import signal
from typing import Optional


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

    elif action == "ping":
        return {"success": True, "message": "pong"}

    elif action == "status":
        return {
            "success": True,
            "presidio_available": analyzer is not None,
            "version": "0.1.0",
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
