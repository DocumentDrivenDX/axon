---
description: Perform a thorough code review with security focus
---

# Code Review

Perform a thorough code review with focus on security, quality, and best practices.

## Review Focus Areas

### 1. Security (Always First)
- OWASP Top 10 vulnerabilities
- Input validation and sanitization
- Authentication and authorization issues
- Injection attacks (SQL, command, XSS)
- Sensitive data exposure

### 2. Code Quality
- Complexity metrics (cyclomatic complexity < 10)
- Maintainability and readability
- Proper error handling
- Resource management and leaks
- Race conditions and concurrency

### 3. Testing
- Test coverage >= 80% for new code
- Edge cases and error paths covered
- Integration points tested

### 4. Documentation
- Public APIs documented
- Complex logic commented
- README updated for new features

## Review Checklist

- [ ] No security vulnerabilities
- [ ] Comprehensive error handling
- [ ] Properly tested (>80% coverage)
- [ ] Performance implications considered
- [ ] Documentation complete
- [ ] No code duplication (DRY)
- [ ] Follows project coding standards
- [ ] Breaking changes documented
- [ ] Logging and monitoring in place

## Feedback Format

### Critical Issues (Must Fix)
Security vulnerabilities, data loss risks, breaking bugs

### Major Issues (Should Fix)
Missing error handling, performance problems, test gaps

### Minor Issues & Suggestions
Style improvements, optional refactoring, naming suggestions

What code would you like me to review?
