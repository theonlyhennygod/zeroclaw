#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== ZeroClaw Google Gemini Validation Script ===${NC}"

# Check for cargo
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo is not installed or not in PATH.${NC}"
    exit 1
fi

# Get API Key
if [ -z "$1" ]; then
    echo -n "Enter your Google Gemini API Key: "
    read -r API_KEY
else
    API_KEY="$1"
fi

if [ -z "$API_KEY" ]; then
    echo -e "${RED}Error: API Key is required.${NC}"
    exit 1
fi

echo -e "\n${GREEN}Testing Gemini connection...${NC}"
echo "Running: cargo run -- agent --provider gemini --model gemini-3-flash-preview --message 'Hello! Please creative a haiku about rust programming.'"

# Run the agent with explicit provider and key via ENV
# We use gemini-1.5-flash as a default model to test
ZEROCLAW_API_KEY="$API_KEY" cargo run --quiet -- agent \
    --provider gemini \
    --model gemini-3-flash-preview \
    --message "Hello! Please create a haiku about rust programming."

if [ $? -eq 0 ]; then
    echo -e "\n${GREEN}✅ Success! Gemini provider is working.${NC}"
else
    echo -e "\n${RED}❌ Failed to validate Gemini provider.${NC}"
fi
