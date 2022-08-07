curl -X POST "https://api.telegram.org/$TG/sendMessage"\
     -H "Content-Type: application/json"\
     -d '{"chat_id": '$CHAT', "text": '"$1"', "parse_mode": "HTML"}'