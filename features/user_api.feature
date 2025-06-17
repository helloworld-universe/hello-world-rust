# features/user_api.feature

Feature: User API Endpoint
  As an application client
  I want to retrieve user information
  So I can confirm the API is functioning correctly

  Scenario: Retrieve a valid user profile
    Given the server is running on "http://127.0.0.1:3000"
    When I send a GET request to "/user"
    Then the response status code should be 200
    And the response body should match JSON:
      """
      {
        "id": 1,
        "name": "Alice"
      }
      """
