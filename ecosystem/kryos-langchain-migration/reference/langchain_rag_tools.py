"""
reference/langchain_rag_tools.py
Canonical LangChain RAG + tool-use chain (diff anchor -- NOT run).

This file demonstrates the Python/LangChain idioms that kryos-langchain-migration
ports to governed Kryos. It is intentionally minimal (~70 lines) so each construct
maps 1-to-1 with a Kryos counterpart in src/main.kry.

Requires: langchain, langchain-openai, faiss-cpu (or chroma)
  pip install langchain langchain-openai faiss-cpu
"""

from typing import Optional
from langchain.agents import AgentExecutor, create_openai_tools_agent
from langchain.tools import tool
from langchain_openai import ChatOpenAI, OpenAIEmbeddings
from langchain.vectorstores import FAISS
from langchain.docstore.document import Document
from langchain.prompts import ChatPromptTemplate, MessagesPlaceholder
from langchain.schema import StrOutputParser

# ---------------------------------------------------------------------------
# 1. Build the corpus (knowledge base)
# ---------------------------------------------------------------------------
DOCS = [
    Document(page_content="Kryos is a capability-safe programming language.",
             metadata={"source": "doc:001"}),
    Document(page_content="Every Kryos function declares its capability set with @capabilities.",
             metadata={"source": "doc:002"}),
    Document(page_content="The @budget attribute caps token and API call spend at compile time.",
             metadata={"source": "doc:003"}),
]

# ---------------------------------------------------------------------------
# 2. Embed + index (FAISS in-process) -- silent citation drop risk here:
#    the Document.metadata["source"] is attached loosely; nothing ENFORCES
#    that every returned chunk carries it through the answer generation step.
# ---------------------------------------------------------------------------
embeddings = OpenAIEmbeddings()
vectorstore = FAISS.from_documents(DOCS, embeddings)
retriever = vectorstore.as_retriever(search_kwargs={"k": 2})


# ---------------------------------------------------------------------------
# 3. Tool definitions -- untyped strings, no capability annotation
# ---------------------------------------------------------------------------
@tool
def calculator(expression: str) -> str:
    """Evaluate a simple arithmetic expression: '2 + 3 * 4'."""
    try:
        result = eval(expression, {"__builtins__": {}})  # nosec (demo only)
        return str(result)
    except Exception as exc:
        return f"error: {exc}"


@tool
def retrieve_docs(query: str) -> str:
    """Retrieve relevant knowledge-base chunks for the query."""
    docs = retriever.invoke(query)
    # BUG CLASS: citation source is concatenated as plain text -- if the
    # downstream chain truncates or reformats this, sources are silently lost.
    return "\n\n".join(
        f"[{d.metadata.get('source','?')}] {d.page_content}" for d in docs
    )


# ---------------------------------------------------------------------------
# 4. Agent -- unbounded loop, no budget enforcement
# ---------------------------------------------------------------------------
llm = ChatOpenAI(model="gpt-4o-mini", temperature=0)
tools = [calculator, retrieve_docs]

prompt = ChatPromptTemplate.from_messages([
    ("system", "You are a precise analyst. Use retrieve_docs for knowledge questions and calculator for arithmetic."),
    MessagesPlaceholder(variable_name="chat_history", optional=True),
    ("human", "{input}"),
    MessagesPlaceholder(variable_name="agent_scratchpad"),
])

agent = create_openai_tools_agent(llm, tools, prompt)
# BUG CLASS: max_iterations is a soft hint; the loop can still run unbounded
# if the agent ignores it. No compile-time @budget(calls=N) equivalent.
agent_executor = AgentExecutor(agent=agent, tools=tools, verbose=True, max_iterations=5)


# ---------------------------------------------------------------------------
# 5. Run -- Python runtime errors, not compile errors
# ---------------------------------------------------------------------------
def run_agent(user_query: str) -> Optional[str]:
    # BUG CLASS: if the API key is missing, this raises at RUNTIME with a
    # generic AuthenticationError -- not caught at compile time.
    result = agent_executor.invoke({"input": user_query})
    return result.get("output")


if __name__ == "__main__":
    answer = run_agent("What does @capabilities do in Kryos? Also, what is 23% of 4400?")
    print("Answer:", answer)
