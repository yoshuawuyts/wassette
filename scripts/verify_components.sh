#!/bin/bash

SERVER_URL="http://localhost:9001"
RESPONSE=$(npx @modelcontextprotocol/inspector --cli $SERVER_URL --method tools/list)

if [ $? -ne 0 ]; then
    echo "Error: Failed to get response from server."
    exit 1
fi

if echo "$RESPONSE" | jq -e '.tools[] | select(.name == "load-component")' > /dev/null; then
    echo -e "\n✅ 'load-component' tool found"
else
    echo -e "\n❌ 'load-component' tool not found!"
fi

if echo "$RESPONSE" | jq -e '.tools[] | select(.name == "unload-component")' > /dev/null; then
    echo "✅ 'unload-component' tool found"
else
    echo "❌ 'unload-component' tool not found!"
fi

# now call load-component on fetch
echo -e "\nCalling load-component on fetch..."
npx @modelcontextprotocol/inspector --cli $SERVER_URL --method tools/call --tool-name load-component --params id=fetch path=target/wasm32-wasip2/release/fetch_rs.wasm

RESPONSE=$(npx @modelcontextprotocol/inspector --cli $SERVER_URL --method tools/list)

# number of tools should be 3
if [ $(echo "$RESPONSE" | jq '.tools | length') -eq 3 ]; then
    echo -e "\n✅ Number of tools is 3"
else
    echo -e "\n❌ Number of tools is not 3!"
fi

if echo "$RESPONSE" | jq -e '.tools[] | select(.name == "fetch")' > /dev/null; then
    echo -e "\n✅ 'fetch' tool found"
else
    echo -e "\n❌ 'fetch' tool not found!"
fi

# now call unload-component on fetch
echo -e "\nCalling unload-component on fetch..."
npx @modelcontextprotocol/inspector --cli $SERVER_URL --method tools/call --tool-name unload-component --params id=fetch

RESPONSE=$(npx @modelcontextprotocol/inspector --cli $SERVER_URL --method tools/list)

# number of tools should be 2
if [ $(echo "$RESPONSE" | jq '.tools | length') -eq 2 ]; then
    echo -e "\n✅ Number of tools is 2"
else
    echo -e "\n❌ Number of tools is not 2!"
fi

echo -e "\nVerification complete." 